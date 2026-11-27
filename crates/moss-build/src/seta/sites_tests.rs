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
// The live generation
// ============================================================================

use crate::seta::client::tests::{signed_requests, StandIn};

fn client_for(server: &StandIn) -> crate::seta::client::MossSetaClient {
    crate::seta::client::MossSetaClient::with_identity_and_url(&crate::identity::Identity::generate().unwrap(), &server.url())
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
        let throughput = crate::seta::upload_policy::Throughput::new(1);
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

/// The client ignores response fields it does not know, so a newer server can add them.
#[test]
fn a_response_with_an_unknown_field_still_deserializes() {
    let json = r#"{"id": "her-blog", "url": "https://her-blog.mosspub.com", "added_later": {"x": 1}}"#;
    let resp: RegisterSiteResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.site_id, "her-blog");
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
