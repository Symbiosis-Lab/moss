use super::*;

#[test]
fn test_register_site_response_deserializes() {
    // New-site shape (201): { id, url }
    let json = r#"{"id": "her-blog", "url": "https://her-blog.mosspub.com"}"#;
    let resp: RegisterSiteResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.site_id, "her-blog");
    assert_eq!(resp.url.as_deref(), Some("https://her-blog.mosspub.com"));
}

#[test]
fn test_register_site_response_deserializes_idempotent_shape() {
    // Already-owned shape (200): { site_id } — no `id`, no `url`.
    let json = r#"{"site_id": "her-blog"}"#;
    let resp: RegisterSiteResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.site_id, "her-blog");
    assert_eq!(resp.url, None);
}

#[test]
fn test_sync_manifest_response_deserializes() {
    // Older seta: no server_total on the wire → None, not a parse error.
    let json = r#"{"need": ["index.html", "about.html"], "remove": ["old.html"]}"#;
    let resp: SyncManifestResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.need, vec!["index.html", "about.html"]);
    assert_eq!(resp.remove, vec!["old.html"]);
    assert_eq!(resp.server_total, None);
}

#[test]
fn test_sync_manifest_response_deserializes_with_server_total() {
    // New seta: server_total = TOTAL live-manifest file count (unfiltered,
    // includes _moss/ internal assets).
    let json = r#"{"need": ["index.html"], "remove": ["old.html"], "server_total": 366}"#;
    let resp: SyncManifestResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.need, vec!["index.html"]);
    assert_eq!(resp.remove, vec!["old.html"]);
    assert_eq!(resp.server_total, Some(366));
}

#[test]
fn test_commit_response_deserializes() {
    let json = r#"{"url": "https://her-blog.mosspub.com", "files_updated": 5, "files_removed": 1, "timestamp": 1711756800}"#;
    let resp: CommitResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.url, "https://her-blog.mosspub.com");
    assert_eq!(resp.files_updated, 5);
    assert_eq!(resp.files_removed, 1);
    assert_eq!(resp.timestamp, 1711756800);
}

#[test]
fn commit_sync_body_includes_generation_id() {
    let manifest: std::collections::HashMap<String, String> = {
        let mut m = std::collections::HashMap::new();
        m.insert("index.html".to_string(), "100644:abc".to_string());
        m
    };
    let generation_id = "abc123def456abcd";
    let body = serde_json::json!({
        "manifest": manifest,
        "generation_id": generation_id,
    });
    assert_eq!(
        body["generation_id"].as_str().unwrap(),
        generation_id,
        "commit_sync wire body must include generation_id field"
    );
    assert!(
        body["manifest"].is_object(),
        "manifest field must be present"
    );
}

#[test]
fn test_site_data_response_deserializes() {
    // Subscribers list present
    let json = r#"{"subscribers": ["a@example.com", "b@example.com"]}"#;
    let resp: SiteDataResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        resp.subscribers.as_deref(),
        Some(["a@example.com".to_string(), "b@example.com".to_string()].as_slice())
    );

    // Empty response
    let resp_empty: SiteDataResponse = serde_json::from_str(r#"{}"#).unwrap();
    assert!(resp_empty.subscribers.is_none());

    // Legacy / unknown fields (analytics, comments) are ignored —
    // serde skips them by default. This guards against server-side
    // additions forcing a client deploy.
    let json_with_extras =
        r#"{"analytics":{"views":100},"comments":[{"id":1}],"subscribers":["x@example.com"]}"#;
    let resp_extras: SiteDataResponse = serde_json::from_str(json_with_extras).unwrap();
    assert_eq!(
        resp_extras.subscribers.as_deref(),
        Some(["x@example.com".to_string()].as_slice())
    );
}

// ============================================================================
// get_live_generation tests — mock HTTP server (TCP listener pattern from
// client.rs: request_reverify tests)
// ============================================================================

/// Helper: spin up a minimal HTTP/1.1 server that returns a fixed response
/// for a single connection.
async fn serve_once(response_bytes: &'static [u8]) -> std::net::SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(response_bytes).await.unwrap();
        stream.shutdown().await.ok();
    });
    addr
}

/// 404 → Ok(None): old server, no /generation endpoint.
#[tokio::test]
async fn test_get_live_generation_404_returns_none() {
    let addr = serve_once(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}",
    )
    .await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let result = client.get_live_generation("her-blog").await;
    assert!(
        result.is_ok(),
        "404 must produce Ok, got: {:?}",
        result.err()
    );
    assert!(result.unwrap().is_none(), "404 must produce Ok(None)");
}

/// 200 with null generation_id → Ok(None): site exists but never deployed.
#[tokio::test]
async fn test_get_live_generation_200_null_returns_none() {
    let addr = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 41\r\nContent-Type: application/json\r\n\r\n{\"generation_id\":null,\"deployed_at\":null}"
        ).await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let result = client.get_live_generation("her-blog").await;
    assert!(
        result.is_ok(),
        "200+null must produce Ok, got: {:?}",
        result.err()
    );
    assert!(result.unwrap().is_none(), "200+null must produce Ok(None)");
}

// ============================================================================
// list_upload_sessions — the resume handshake (I2)
// ============================================================================

/// **404 must mean "no sessions", not "publish failed".**
///
/// An older seta has no `/uploads` route and Hono answers 404.
/// `from_failed_response` turns every non-2xx into a terminal `SetaError` and
/// `is_transient` excludes 404, so without the explicit arm a new client would
/// hard-fail EVERY publish against a server that has not been upgraded yet.
#[tokio::test]
async fn list_upload_sessions_treats_404_as_no_sessions() {
    let addr = serve_once(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nContent-Type: text/plain\r\n\r\nNot Found",
    )
    .await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let result = client
        .list_upload_sessions("her-blog", "abc123def456abcd")
        .await;
    assert!(
        result.is_ok(),
        "a server too old for the resume endpoint must not fail the publish, got: {:?}",
        result.err()
    );
    assert!(result.unwrap().is_empty(), "404 must produce an empty list");
}

/// The wire shape, pinned against moss-seta's `listUploadSessions`.
#[tokio::test]
async fn list_upload_sessions_parses_the_server_shape() {
    let body = br#"[{"uploadId":"u-1","filePath":"audio/dreamin.mp3","size":8910888,"offset":4194304}]"#;
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    let addr = serve_once(Box::leak(bytes.into_boxed_slice())).await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let sessions = client
        .list_upload_sessions("her-blog", "abc123def456abcd")
        .await
        .expect("a 200 must parse");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].upload_id, "u-1");
    assert_eq!(sessions[0].file_path, "audio/dreamin.mp3");
    assert_eq!(sessions[0].size, 8_910_888);
    assert_eq!(sessions[0].offset, 4_194_304);
}

// ============================================================================
// CommitResponse: generation_id field (Piece A)
// ============================================================================

/// Server echoes generation_id → CommitResponse parses it as Some(id).
#[test]
fn commit_response_deserializes_with_generation_id() {
    let json = r#"{
            "url": "https://her-blog.mosspub.com",
            "files_updated": 3,
            "files_removed": 0,
            "timestamp": 1711756800,
            "generation_id": "abc123def456abcd"
        }"#;
    let resp: CommitResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.generation_id, Some("abc123def456abcd".to_string()));
    assert_eq!(resp.url, "https://her-blog.mosspub.com");
}

/// Old server omits generation_id → CommitResponse still deserializes (serde default → None).
#[test]
fn commit_response_deserializes_without_generation_id() {
    let json = r#"{
            "url": "https://her-blog.mosspub.com",
            "files_updated": 2,
            "files_removed": 1,
            "timestamp": 1711756800
        }"#;
    let resp: CommitResponse = serde_json::from_str(json).unwrap();
    assert!(
        resp.generation_id.is_none(),
        "missing generation_id must default to None, got: {:?}",
        resp.generation_id
    );
}

/// 200 with a real generation_id → Ok(Some(id)).
#[tokio::test]
async fn test_get_live_generation_200_with_id_returns_some() {
    let body = br#"{"generation_id":"abc123def456abcd","deployed_at":1718000000}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n",
        body.len()
    );
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    // Leak to get a 'static ref for serve_once; this is test-only.
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let addr = serve_once(leaked).await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let result = client.get_live_generation("her-blog").await;
    assert!(
        result.is_ok(),
        "200+id must produce Ok, got: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Some("abc123def456abcd".to_string()));
}

// ============================================================================
// Generation-id header threading tests (Stage 2 showstopper fix)
//
// Verify that sync_manifest, upload_file, and the chunked create-session POST
// each send `X-Moss-Generation` with the caller-supplied generation_id, so the
// server routes all writes into the correct generation directory instead of
// defaulting to `_default/`.
// ============================================================================

/// Helper to extract a named header value from a raw HTTP/1.1 request byte slice.
fn extract_header(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let needle = format!("{}:", name.to_ascii_lowercase());
    text.lines().find_map(|line| {
        if line.to_ascii_lowercase().starts_with(&needle) {
            Some(line[line.find(':').unwrap() + 1..].trim().to_string())
        } else {
            None
        }
    })
}

/// sync_manifest must send `X-Moss-Generation` with the given generation_id.
///
/// Mock: single connection, POST /sync → 200 `{"need":[],"remove":[]}`.
#[tokio::test]
async fn sync_manifest_sends_generation_header() {
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();

    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 8192];
        let mut raw = Vec::new();
        loop {
            let n = stream.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 23\r\n\r\n{\"need\":[],\"remove\":[]}";
        stream.write_all(resp).await.ok();
        stream.shutdown().await.ok();
        let _ = tx.send(raw);
    });

    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let manifest = std::collections::HashMap::new();
    client
        .sync_manifest("her-blog", &manifest, "deadbeef12345678")
        .await
        .expect("sync_manifest must succeed against mock server");

    let raw = rx.await.expect("mock server must capture request");
    let gen_header = extract_header(&raw, "x-moss-generation");
    assert!(
        gen_header.is_some(),
        "sync_manifest POST must include X-Moss-Generation header; request was:\n{}",
        String::from_utf8_lossy(&raw)
    );
    assert_eq!(
        gen_header.unwrap(),
        "deadbeef12345678",
        "X-Moss-Generation must equal the generation_id passed to sync_manifest"
    );
}

/// upload_file (PUT /files) must send `X-Moss-Generation` with the given generation_id.
///
/// Mock: single connection, PUT /files/... → 200 (empty body).
#[tokio::test]
async fn upload_file_sends_generation_header() {
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();

    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = stream.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
        stream.write_all(resp).await.ok();
        stream.shutdown().await.ok();
        let _ = tx.send(raw);
    });

    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    client
        .upload_file(
            "her-blog",
            "index.html",
            b"<html></html>".to_vec(),
            "cafebabe87654321",
            None,
        )
        .await
        .expect("upload_file must succeed against mock server");

    let raw = rx.await.expect("mock server must capture request");
    let gen_header = extract_header(&raw, "x-moss-generation");
    assert!(
        gen_header.is_some(),
        "upload_file PUT must include X-Moss-Generation header; request was:\n{}",
        String::from_utf8_lossy(&raw)
    );
    assert_eq!(
        gen_header.unwrap(),
        "cafebabe87654321",
        "X-Moss-Generation on upload_file must equal the passed generation_id"
    );
}

/// Chunked create-session POST must send `X-Moss-Generation`; the complete
/// POST must still send `X-Moss-Content-Hash` (and NOT be expected to carry
/// X-Moss-Generation — the server reads it from the upload session).
///
/// Reuses the 3-connection mock pattern from `upload_file_chunked_sends_content_hash_header`.
#[tokio::test]
async fn upload_file_chunked_create_session_sends_generation_header() {
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Two channels: one for the create-session POST headers, one for the
    // complete POST headers (so we can assert X-Moss-Content-Hash is intact).
    let (tx_create, rx_create) = tokio::sync::oneshot::channel::<Vec<u8>>();
    let (tx_complete, rx_complete) = tokio::sync::oneshot::channel::<Vec<u8>>();

    tokio::spawn(async move {
        // conn 0: GET /uploads (resume handshake) → nothing staged
        let (stream, _) = listener.accept().await.unwrap();
        crate::test_mock_http_conn(
            stream,
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]",
        )
        .await;

        // conn 1: POST /upload (create-session)
        let create_resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 29\r\n\r\n{\"uploadId\":\"test-upload-id\"}";
        let (stream, _) = listener.accept().await.unwrap();
        let create_raw = crate::test_mock_http_conn(stream, create_resp).await;
        let _ = tx_create.send(create_raw);

        // conn 2: PATCH chunk
        let (stream, _) = listener.accept().await.unwrap();
        crate::test_mock_http_conn(stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;

        // conn 3: POST complete
        let (stream, _) = listener.accept().await.unwrap();
        let complete_raw =
            crate::test_mock_http_conn(stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
        let _ = tx_complete.send(complete_raw);
    });

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let file_bytes: Vec<u8> = vec![0x01, 0x02, 0x03];
    let file_path = tmp.path().join("gen-test.bin");
    std::fs::write(&file_path, &file_bytes).unwrap();

    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );

    client
        .upload_file_chunked(
            "her-blog",
            "gen-test.bin",
            &file_path,
            file_bytes.len() as u64,
            "1122334455667788",
            &crate::seta::upload_policy::Throughput::new(),
            None,
        )
        .await
        .expect("upload_file_chunked must succeed against mock server");

    // The create-session POST must carry X-Moss-Generation.
    let create_raw = rx_create.await.expect("mock must send create request");
    let gen_on_create = extract_header(&create_raw, "x-moss-generation");
    assert!(
        gen_on_create.is_some(),
        "chunked create-session POST must include X-Moss-Generation; request was:\n{}",
        String::from_utf8_lossy(&create_raw)
    );
    assert_eq!(
        gen_on_create.unwrap(),
        "1122334455667788",
        "X-Moss-Generation on chunked create-session must equal the passed generation_id"
    );

    // The complete POST must still carry X-Moss-Content-Hash (C3, unchanged).
    let complete_raw = rx_complete.await.expect("mock must send complete request");
    let hash_on_complete = extract_header(&complete_raw, "x-moss-content-hash");
    assert!(
        hash_on_complete.is_some(),
        "chunked complete POST must still include X-Moss-Content-Hash; request was:\n{}",
        String::from_utf8_lossy(&complete_raw)
    );
    // The complete POST must NOT carry X-Moss-Generation (server reads it from session).
    let gen_on_complete = extract_header(&complete_raw, "x-moss-generation");
    assert!(
        gen_on_complete.is_none(),
        "chunked complete POST must NOT include X-Moss-Generation (server reads it from session); \
             got: {:?}",
        gen_on_complete
    );
}

// ============================================================================
// Transient-failure retry tests (B1)
//
// A seta redeploy mid-publish returned 502 for 8 uploads in the same second
// and aborted a 761-file publish. `upload_file` must survive a transient 5xx
// by retrying — and each retry must RE-SIGN, because the Authorization header
// signs "{timestamp}:{payload}" and seta rejects a clock skew > 300s.
// ============================================================================

/// Decode the `timestamp` claim out of a `Nostr <base64(json)>` auth header.
fn extract_timestamp_from_auth_header(header: &str) -> u64 {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    let b64 = header
        .strip_prefix("Nostr ")
        .expect("auth header must start with 'Nostr '");
    let json = BASE64
        .decode(b64)
        .expect("auth header must be valid base64");
    let value: serde_json::Value = serde_json::from_slice(&json).expect("auth header must be JSON");
    value
        .get("timestamp")
        .and_then(|v| v.as_u64())
        .expect("signed request must carry a timestamp")
}

/// Mock HTTP server that answers connection *n* with `responses[n]` (the last
/// entry repeats forever), captures every raw request, and optionally stalls
/// before answering the first one. Every response closes the connection, so
/// one connection == one request attempt and the captured count is exact.
async fn serve_retry_sequence(
    responses: Vec<&'static str>,
    first_response_delay: std::time::Duration,
) -> (
    std::net::SocketAddr,
    std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
    let captured_bg = std::sync::Arc::clone(&captured);

    tokio::spawn(async move {
        let mut n = 0usize;
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let read = stream.read(&mut buf).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..read]);
                if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            captured_bg.lock().unwrap().push(raw);
            if n == 0 && !first_response_delay.is_zero() {
                tokio::time::sleep(first_response_delay).await;
            }
            let resp = responses[n.min(responses.len() - 1)];
            stream.write_all(resp.as_bytes()).await.ok();
            stream.shutdown().await.ok();
            n += 1;
        }
    });

    (addr, captured)
}

const RESP_502: &str = "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const RESP_403: &str = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const RESP_204: &str = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";

/// A transient 502 must be retried, and the retry must be freshly signed.
///
/// The mock stalls 2s before answering the first attempt, so a *cached*
/// Authorization header would reproduce the first attempt's timestamp
/// verbatim while a genuine re-sign is necessarily ≥2s newer. Asserting
/// `ts2 > ts1` therefore proves re-signing, unlike "the headers differ"
/// (which a nonce or any other varying field would also satisfy).
#[tokio::test]
async fn upload_file_retries_transient_5xx() {
    let (addr, captured) =
        serve_retry_sequence(vec![RESP_502, RESP_204], std::time::Duration::from_secs(2)).await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );

    let result = client
        .upload_file("her-blog", "index.html", b"<html></html>".to_vec(), "gen1", None)
        .await;
    assert!(
        result.is_ok(),
        "a transient 502 must be retried to success, got: {:?}",
        result.err()
    );

    let reqs = captured.lock().unwrap().clone();
    assert_eq!(
        reqs.len(),
        2,
        "expected exactly 1 retry after the 502, saw {} requests",
        reqs.len()
    );

    let ts1 = extract_timestamp_from_auth_header(
        &extract_header(&reqs[0], "authorization").expect("attempt 1 must be signed"),
    );
    let ts2 = extract_timestamp_from_auth_header(
        &extract_header(&reqs[1], "authorization").expect("attempt 2 must be signed"),
    );
    assert!(
        ts2 > ts1,
        "the retry must RE-SIGN (seta rejects skew > 300s): attempt 1 signed at {}, retry at {} \
             — an identical timestamp means the Authorization header was replayed",
        ts1,
        ts2
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        now.saturating_sub(ts2) < 60,
        "retry signature must be fresh: ts2={} now={}",
        ts2,
        now
    );
}

/// A 403 is the server's final answer — retrying it wastes time and can
/// hammer an auth-rejecting endpoint. Exactly one request must be sent.
#[tokio::test]
async fn upload_file_does_not_retry_4xx() {
    let (addr, captured) = serve_retry_sequence(vec![RESP_403], std::time::Duration::ZERO).await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );

    let result = client
        .upload_file("her-blog", "index.html", b"<html></html>".to_vec(), "gen1", None)
        .await;
    assert!(result.is_err(), "a 403 must fail, not succeed");
    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "a non-transient 4xx must fail fast with no retries"
    );
}

/// Retries are bounded: a permanently-down origin must not retry forever.
#[tokio::test]
async fn upload_file_gives_up_after_bounded_attempts() {
    let (addr, captured) = serve_retry_sequence(vec![RESP_502], std::time::Duration::ZERO).await;
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );

    let result = client
        .upload_file("her-blog", "index.html", b"<html></html>".to_vec(), "gen1", None)
        .await;
    assert!(
        result.is_err(),
        "an origin that is 502 forever must eventually fail"
    );
    assert_eq!(
        captured.lock().unwrap().len(),
        4,
        "expected 1 try + 3 retries then give up"
    );
}
