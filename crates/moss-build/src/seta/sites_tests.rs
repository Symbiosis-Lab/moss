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
// get_live_generation
// ============================================================================

use crate::seta::client::tests::{signed_requests, StandIn};

fn client_for(server: &StandIn) -> crate::seta::client::MossSetaClient {
    crate::seta::client::MossSetaClient::with_identity_and_url(&crate::identity::Identity::generate().unwrap(), &server.url())
}

/// 404 → Ok(None): old server, no /generation endpoint.
#[tokio::test]
async fn test_get_live_generation_404_returns_none() {
    let server = StandIn::json(404, "{}").await;
    let result = client_for(&server).get_live_generation("her-blog").await;
    assert!(result.as_ref().is_ok_and(Option::is_none), "404 must produce Ok(None), got: {result:?}");
}

/// 200 with null generation_id → Ok(None): site exists but never deployed.
#[tokio::test]
async fn test_get_live_generation_200_null_returns_none() {
    let server = StandIn::json(200, r#"{"generation_id":null,"deployed_at":null}"#).await;
    let result = client_for(&server).get_live_generation("her-blog").await;
    assert!(result.as_ref().is_ok_and(Option::is_none), "200+null must produce Ok(None), got: {result:?}");
}

/// Only seta's own 403 is another owner; `live_generation` still answers it
/// as an error, so its callers go ahead as before.
#[tokio::test]
async fn test_generation_403_is_not_owner_and_live_generation_keeps_the_error() {
    let server = StandIn::json(403, r#"{"error":"Not authorized for this site"}"#).await;
    let client = client_for(&server);
    let result = client.generation("her-blog", None).await;
    assert!(matches!(result, Ok(crate::seta::sites::Generation::NotOwner)), "{result:?}");
    let live = client.live_generation("her-blog").await;
    assert!(matches!(live, Err(crate::seta::client::SetaError::Api { status: 403, .. })), "{live:?}");
}

/// seta's coded 403 is the owner's key not signed in to this site, named by
/// its address; the same words without the code stay another owner.
#[tokio::test]
async fn a_coded_403_is_not_signed_in_and_names_the_address() {
    let server = StandIn::json(403, r#"{"error":"Not authorized for this site","code":"folder_not_signed_in","site":"her-blog.mosspub.com","detail":"This folder isn't signed in to her-blog.mosspub.com."}"#).await;
    let client = client_for(&server);
    let result = client.generation("her-blog", None).await;
    assert!(matches!(&result, Ok(crate::seta::sites::Generation::NotSignedIn { site }) if site == "her-blog.mosspub.com"), "{result:?}");
    let live = client.live_generation("her-blog").await;
    assert!(matches!(live, Err(crate::seta::client::SetaError::NotSignedIn { .. })), "{live:?}");
}

/// A proxy's 403 is not seta saying the key is another account's.
#[tokio::test]
async fn test_generation_403_not_in_setas_words_stays_an_error() {
    let server = StandIn::new(403, &[("content-type", "text/html")], |_, _| "<html><body>Forbidden</body></html>".to_string()).await;
    let result = client_for(&server).generation("her-blog", None).await;
    assert!(matches!(result, Err(crate::seta::client::SetaError::Api { status: 403, .. })), "{result:?}");
}

#[tokio::test]
async fn test_generation_404_is_not_found_and_401_stays_an_error() {
    let server = StandIn::json(404, r#"{"error":"Site not found"}"#).await;
    let result = client_for(&server).generation("her-blog", None).await;
    assert!(matches!(result, Ok(crate::seta::sites::Generation::NotFound)), "{result:?}");
    let server = StandIn::json(401, r#"{"error":"Invalid signature"}"#).await;
    let result = client_for(&server).generation("her-blog", None).await;
    assert!(matches!(result, Err(crate::seta::client::SetaError::Api { status: 401, .. })), "{result:?}");
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
    let server = StandIn::new(404, &[("content-type", "text/plain")], |_, _| "Not Found".into()).await;
    let result = client_for(&server)
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
    let server = StandIn::json(200, r#"[{"uploadId":"u-1","filePath":"audio/dreamin.mp3","size":8910888,"offset":4194304}]"#).await;
    let sessions = client_for(&server)
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

/// 200 with a real generation_id → the id, and when it went live in the
/// server's Unix seconds (what `moss deploy`'s stale-copy check compares).
#[tokio::test]
async fn test_live_generation_200_carries_id_and_deployed_at() {
    let server = StandIn::json(200, r#"{"generation_id":"abc123def456abcd","deployed_at":1718000000}"#).await;
    let result = client_for(&server).live_generation("her-blog").await;
    assert!(
        result.is_ok(),
        "200+id must produce Ok, got: {:?}",
        result.err()
    );
    assert_eq!(
        result.unwrap(),
        crate::seta::sites::LiveGeneration {
            generation_id: Some("abc123def456abcd".to_string()),
            deployed_at: Some(1718000000),
        }
    );
}

// ============================================================================
// Generation-id header threading tests
//
// Verify that sync_manifest, upload_file, and the chunked create-session POST
// each send `X-Moss-Generation` with the caller-supplied generation_id, so the
// server routes all writes into the correct generation directory instead of
// defaulting to `_default/`.
// ============================================================================

/// sync_manifest must send `X-Moss-Generation` with the given generation_id.
#[tokio::test]
async fn sync_manifest_sends_generation_header() {
    let identity = crate::identity::Identity::generate().unwrap();
    let requests = signed_requests(&identity, |c| async move {
        c.sync_manifest("her-blog", &std::collections::HashMap::new(), &std::collections::HashMap::new(), "deadbeef12345678").await
    })
    .await;
    assert_eq!(requests[0].header("x-moss-generation"), Some("deadbeef12345678"));
}

/// The sync sends each file's size beside its manifest entry, so the server
/// can refuse a site over its storage cap before any upload.
#[tokio::test]
async fn sync_manifest_sends_sizes_beside_files() {
    let identity = crate::identity::Identity::generate().unwrap();
    let files = [("a.html".to_string(), "100644:h1".to_string()), ("latest".to_string(), "120000:h2".to_string())].into();
    let sizes = [("a.html".to_string(), 1234u64)].into();
    let requests = signed_requests(&identity, |c| async move { c.sync_manifest("her-blog", &files, &sizes, "g1").await }).await;
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["files"]["latest"], "120000:h2");
    assert_eq!(body["sizes"], serde_json::json!({ "a.html": 1234 }));
}

/// upload_file (PUT /files) must send `X-Moss-Generation` with the given generation_id.
#[tokio::test]
async fn upload_file_sends_generation_header() {
    let identity = crate::identity::Identity::generate().unwrap();
    let requests = signed_requests(&identity, |c| async move {
        c.upload_file("her-blog", "index.html", b"<html></html>".to_vec(), "cafebabe87654321", None).await
    })
    .await;
    assert_eq!(requests[0].header("x-moss-generation"), Some("cafebabe87654321"));
}

/// A chunked upload is four signed requests: the resume handshake, the
/// create-session POST (the only one carrying `X-Moss-Generation`; the server
/// keeps it with the session), the chunk, and the complete POST with the
/// content hash.
#[tokio::test]
async fn upload_file_chunked_signs_each_request_and_sends_the_generation_once() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let file_path = tmp.path().join("gen-test.bin");
    std::fs::write(&file_path, [0x01, 0x02, 0x03]).unwrap();

    let identity = crate::identity::Identity::generate().unwrap();
    let path = file_path.clone();
    let requests = signed_requests(&identity, |c| async move {
        let throughput = crate::seta::upload_policy::Throughput::new();
        c.upload_file_chunked("her-blog", "gen-test.bin", &path, 3, "1122334455667788", &throughput, None).await
    })
    .await;
    let sent: Vec<_> = requests.iter().map(|r| (r.method.as_str(), r.path_and_query.as_str())).collect();
    assert_eq!(sent, [
        ("GET", "/api/sites/her-blog/uploads?generation_id=1122334455667788"),
        ("POST", "/api/sites/her-blog/upload"),
        ("PATCH", "/api/sites/her-blog/upload/u1"),
        ("POST", "/api/sites/her-blog/upload/u1/complete"),
    ]);
    assert_eq!(requests[2].body, [0x01, 0x02, 0x03]);
    assert_eq!(requests[1].header("x-moss-generation"), Some("1122334455667788"));
    assert!(requests[3].header("x-moss-content-hash").is_some());
    assert_eq!(requests[3].header("x-moss-generation"), None);
}

// ============================================================================
// Transient-failure retry tests (B1)
//
// A seta redeploy mid-publish returned 502 for 8 uploads in the same second
// and aborted a 761-file publish. `upload_file` must survive a transient 5xx
// by retrying, and each retry must sign afresh: seta accepts a signed header
// once.
// ============================================================================

/// Mock HTTP server that answers connection *n* with `responses[n]` (the last
/// entry repeats forever) and records every request. Every response
/// closes the connection, so one connection == one request attempt and the
/// captured count is exact.
async fn serve_retry_sequence(
    responses: Vec<&'static str>,
) -> (
    std::net::SocketAddr,
    std::sync::Arc<std::sync::Mutex<Vec<crate::seta::client::tests::Received>>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
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
                // The head, then as many body bytes as it declares.
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let declared = crate::seta::client::tests::Received::from_raw(&raw).header("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                    if raw.len() >= end + 4 + declared {
                        break;
                    }
                }
            }
            captured_bg.lock().unwrap().push(crate::seta::client::tests::Received::from_raw(&raw));
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

/// A transient 502 must be retried, and the retry must be freshly signed: a
/// replayed header would carry the first attempt's nonce, which seta refuses.
#[tokio::test]
async fn upload_file_retries_transient_5xx() {
    let (addr, captured) =
        serve_retry_sequence(vec![RESP_502, RESP_204]).await;
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

    for attempt in &reqs {
        assert_eq!(attempt.signer(), identity.pubkey);
    }
    assert_ne!(reqs[0].header("authorization"), reqs[1].header("authorization"), "the retry replayed the first attempt's header");
}

/// A 403 is the server's final answer — retrying it wastes time and can
/// hammer an auth-rejecting endpoint. Exactly one request must be sent.
#[tokio::test]
async fn upload_file_does_not_retry_4xx() {
    let (addr, captured) = serve_retry_sequence(vec![RESP_403]).await;
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
    let (addr, captured) = serve_retry_sequence(vec![RESP_502]).await;
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

#[test]
fn sync_manifest_response_internal_remove_decides_whether_to_commit() {
    let parse = |json: &str| serde_json::from_str::<SyncManifestResponse>(json).unwrap();
    // Older server: field absent, nothing to do.
    assert!(!parse(r#"{"need": [], "remove": []}"#).needs_commit());
    assert!(!parse(r#"{"need": [], "remove": [], "internal_remove": 0}"#).needs_commit());
    assert!(parse(r#"{"need": [], "remove": [], "internal_remove": 1}"#).needs_commit());
    assert!(parse(r#"{"need": ["a.html"], "remove": []}"#).needs_commit());
    assert!(parse(r#"{"need": [], "remove": ["a.html"]}"#).needs_commit());
}

#[test]
fn event_row_round_trips_the_visitor_code() {
    let json = r#"{"id":7,"ts":1700000000,"path":"/","referrer":null,"country":null,"browser":null,"os":null,"screen":null,"utmSource":null,"utmMedium":null,"event":null,"visitor":"0123456789abcdef"}"#;
    let row: EventRow = serde_json::from_str(json).unwrap();
    assert_eq!(row.visitor.as_deref(), Some("0123456789abcdef"));
    let back = serde_json::to_value(&row).unwrap();
    assert_eq!(back["visitor"], "0123456789abcdef");
}

#[test]
fn event_row_without_visitor_parses_as_none() {
    let json = r#"{"id":7,"ts":1700000000,"path":"/","referrer":null,"country":null,"browser":null,"os":null,"screen":null,"utmSource":null,"utmMedium":null,"event":null}"#;
    let row: EventRow = serde_json::from_str(json).unwrap();
    assert_eq!(row.visitor, None);
}

/// The client ignores response fields it does not know, so a newer server can add them.
#[test]
fn a_response_with_an_unknown_field_still_deserializes() {
    let json = r#"{"url": "https://her-blog.mosspub.com", "files_updated": 5, "files_removed": 1, "timestamp": 1711756800, "added_later": {"x": 1}}"#;
    let resp: CommitResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.files_updated, 5);
}

fn available(server: &StandIn) -> impl std::future::Future<Output = Result<bool, crate::seta::client::SetaError>> + '_ {
    let client = client_for(server);
    async move { client.check_site_id_available("her-blog").await }
}

#[tokio::test]
async fn the_name_check_answers_taken_and_available() {
    let taken = StandIn::json(200, r#"{"available":false}"#).await;
    assert!(matches!(available(&taken).await, Ok(false)));
    let free = StandIn::json(200, r#"{"available":true}"#).await;
    assert!(matches!(available(&free).await, Ok(true)));
}

#[tokio::test]
async fn the_name_check_reports_a_server_failure_instead_of_taken() {
    let server = StandIn::json(500, r#"{"error":"boom"}"#).await;
    let result = available(&server).await;
    assert!(matches!(result, Err(crate::seta::client::SetaError::Api { status: 500, .. })), "{result:?}");
}
