use super::*;

fn test_identity() -> Identity {
    Identity::generate().expect("Failed to generate test identity")
}

/// The shared seta client must build with the system-proxy custom closure
/// set — the regression guard for the reqwest-behind-the-GFW fix. A broken
/// `Proxy::custom` closure type or a missing reqwest `socks` feature would
/// fail to compile; a TLS-init failure would panic in `.expect(..)`. Building
/// it here proves the `.proxy(system_proxy())` wiring is sound and that every
/// seta call (deploy `sync`/`upload`/`commit` + Send logs) gets a proxied
/// client. reqwest invokes the resolver lazily per request, so construction
/// alone exercises the wiring without needing a live proxy.
#[test]
fn build_seta_http_client_builds_with_system_proxy() {
    let _client = build_seta_http_client();
}

/// Send Logs must fail fast rather than inherit the shared 120s upload
/// ceiling — a 2-minute "Sending logs…" spinner reads to the user as a hang.
#[test]
fn send_logs_request_timeout_is_tighter_than_the_shared_default() {
    assert!(SEND_LOGS_REQUEST_TIMEOUT < SETA_REQUEST_TIMEOUT);
    assert!(SEND_LOGS_REQUEST_TIMEOUT <= std::time::Duration::from_secs(45));
}

/// The challenge copy is the whole point of the incident fix: it is what a
/// permanently-blocked user reads. Pin it on `SetaError::Challenge`'s
/// Display, because that is the string the `e.to_string()` fallbacks across
/// identity/domain commands actually surface.
#[test]
fn challenge_copy_never_tells_the_user_to_retry() {
    let msg = SetaError::Challenge { status: 403 }.to_string();
    assert!(
        !msg.contains("<!DOCTYPE"),
        "raw HTML leaked into message: {msg}"
    );
    assert!(
        !msg.contains("challenge-platform"),
        "raw HTML leaked into message: {msg}"
    );
    assert!(
        msg.to_lowercase().contains("security check"),
        "expected a friendly security-check reason, got: {msg}"
    );
    // A managed challenge is unsolvable by a native client — the block is
    // permanent, so the copy must never suggest waiting or retrying (in
    // the 2026-07-21 incident the old "usually temporary, try again"
    // wording drove ~21 futile manual retries).
    assert!(
        msg.contains("retrying won't fix"),
        "copy must say retrying is futile: {msg}"
    );
    assert!(
        !msg.to_lowercase().contains("temporary") && !msg.to_lowercase().contains("try again"),
        "copy must not call a permanent block temporary: {msg}"
    );
    assert!(
        msg.contains("up to date"),
        "copy must point at the actionable fixes (update moss): {msg}"
    );
    assert!(
        msg.contains("different network"),
        "copy must point at the actionable fixes (change network): {msg}"
    );
    assert!(
        !msg.contains("Failed to send logs") && !msg.contains("403"),
        "Display must not re-add a prefix or status (the toast layer does): {msg}"
    );
}

/// The `cf-mitigated: challenge` header is Cloudflare's authoritative
/// signal and must be honored on any status.
#[test]
fn test_challenge_detected_via_cf_header() {
    let body = "<!DOCTYPE html><html lang=\"en-US\"><head><title>Just a moment...</title>\
            </head><body><script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/chl_page\">\
            </script></body></html>";
    assert!(is_edge_challenge(Some("challenge"), body));
}

/// Even without the `cf-mitigated` header, a body carrying Cloudflare's
/// challenge markers must be recognized (defense in depth — the header is
/// not guaranteed on every challenge variant).
#[test]
fn test_challenge_detected_via_body_markers() {
    let body = "<!DOCTYPE html><html><head></head><body>\
            <script>window._cf_chl_opt={cType:'managed'};</script></body></html>";
    assert!(is_edge_challenge(None, body));
}

/// A genuine moss-seta JSON error must pass through unchanged so support
/// still sees the authored reason — never mislabeled as a security check.
#[test]
fn test_json_api_error_is_not_a_challenge() {
    let body = r#"{"error":"rate limit exceeded"}"#;
    assert!(!is_edge_challenge(None, body));
    assert_eq!(extract_api_error_message(body), "rate limit exceeded");
}

/// A 403 carrying an authored JSON body — the status most confusable with a
/// challenge — must NOT be mislabeled. Detection is by header/markers, never
/// by status code.
#[test]
fn test_403_json_error_is_not_mislabeled_as_a_challenge() {
    let body = r#"{"error":"forbidden"}"#;
    assert!(
        !is_edge_challenge(None, body),
        "a real 403 API error must not be mislabeled as a challenge"
    );
    assert_eq!(extract_api_error_message(body), "forbidden");
}

/// Only `cf-mitigated: challenge` counts. A different (hypothetical future)
/// mitigation value must not hijack a real API error into a challenge.
#[test]
fn test_non_challenge_cf_mitigated_value_is_not_a_challenge() {
    let body = r#"{"error":"bad request"}"#;
    assert!(!is_edge_challenge(Some("dynamic"), body));
    assert_eq!(extract_api_error_message(body), "bad request");
}

/// Serve ONE canned HTTP response on a throwaway localhost port and return
/// its base URL. Lets a test drive a real request through the shared client
/// (proxy, User-Agent, `from_failed_response` and all) rather than asserting
/// on a hand-built `SetaError`.
async fn serve_once(response: String) -> (String, tokio::task::JoinHandle<()>) {
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await.unwrap();
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.ok();
    });
    (format!("http://{}", addr), handle)
}

/// A Cloudflare managed-challenge response, in the shape the edge really
/// sends it.
fn challenge_response(status_line: &str) -> String {
    let body = "<!DOCTYPE html><html><head><title>Just a moment...</title></head>\
            <body><script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/chl_page\"></script></body></html>";
    format!(
            "HTTP/1.1 {}\r\ncf-mitigated: challenge\r\ncf-ray: 8f0a1b2c3d4e5f60-SJC\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
            status_line,
            body.len(),
            body
        )
}

/// `fetch_stripe_publishable_key` used to hand-roll its own `SetaError::Api`
/// straight from `resp.text()`, bypassing `from_failed_response`. That made
/// it the one endpoint with no cf-ray in the log, no length cap, and no
/// challenge classification — so a challenge page reached the payment UI as
/// a wall of raw HTML. Every endpoint goes through the one door.
#[tokio::test]
async fn the_stripe_key_endpoint_classifies_a_challenge_like_every_other() {
    let (url, handle) = serve_once(challenge_response("403 Forbidden")).await;
    let client = MossSetaClient::with_url(&url);
    let err = client
        .fetch_stripe_publishable_key()
        .await
        .expect_err("a 403 challenge must fail");
    handle.await.unwrap();

    assert!(
        matches!(err, SetaError::Challenge { status: 403 }),
        "the stripe endpoint must use the shared failed-response path, got: {err:?}"
    );
    let shown = err.to_string();
    assert!(
        !shown.contains("<!DOCTYPE") && !shown.contains("challenge-platform"),
        "raw challenge HTML leaked to the payment UI: {shown}"
    );
}

/// `link_custom_domain` hand-rolled its 409 arm from the raw body, so the
/// cross-site-conflict message the orchestrator shows the user was an
/// uncapped JSON blob. Routed through the shared path it becomes the
/// server's authored sentence.
#[tokio::test]
async fn the_domain_link_conflict_surfaces_the_authored_message() {
    let body = r#"{"error":"Domain is already linked to another site"}"#;
    let resp = format!(
        "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let (url, handle) = serve_once(resp).await;
    let identity = test_identity();
    let client = MossSetaClient::with_identity_and_url(&identity, &url);
    let err = client
        .link_custom_domain("site", "example.com")
        .await
        .expect_err("a 409 conflict must fail");
    handle.await.unwrap();

    match err {
        SetaError::Api {
            status: 409,
            message,
        } => assert_eq!(
            message, "Domain is already linked to another site",
            "the 409 arm must extract the authored message, not dump the raw body"
        ),
        other => panic!("expected an Api 409, got: {other:?}"),
    }
}

/// The same endpoint must not lose challenge classification either — a
/// challenge on the domain-link call is permanent, and the orchestrator
/// routes anything that is not a 409 into a "transient, back off and retry"
/// job state.
#[tokio::test]
async fn the_domain_link_endpoint_classifies_a_challenge() {
    let (url, handle) = serve_once(challenge_response("403 Forbidden")).await;
    let identity = test_identity();
    let client = MossSetaClient::with_identity_and_url(&identity, &url);
    let err = client
        .link_custom_domain("site", "example.com")
        .await
        .expect_err("a 403 challenge must fail");
    handle.await.unwrap();

    assert!(
        matches!(err, SetaError::Challenge { status: 403 }),
        "expected a challenge, got: {err:?}"
    );
    assert!(!err.is_transient(), "a challenge must not be retried");
}

/// The end-to-end wiring: a real challenge response over the wire must
/// arrive at the caller as `SetaError::Challenge`, not as an `Api` the
/// caller has to re-guess from the status. This is the link that keeps
/// `domain::commands` from answering a permanent block with retry advice.
#[tokio::test]
async fn a_challenge_response_reaches_the_caller_as_challenge() {
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await.unwrap();
        // Shape of a real Cloudflare managed-challenge response: 403,
        // `cf-mitigated: challenge`, an unsolvable HTML interstitial.
        let body = "<!DOCTYPE html><html><head><title>Just a moment...</title></head>\
                <body><script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/chl_page\"></script></body></html>";
        let resp = format!(
                "HTTP/1.1 403 Forbidden\r\ncf-mitigated: challenge\r\ncf-ray: 8f0a1b2c3d4e5f60-SJC\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
        stream.write_all(resp.as_bytes()).await.unwrap();
        stream.shutdown().await.ok();
    });

    let url = format!("http://{}", addr);
    let client = MossSetaClient::with_url(&url);
    let err = client
        .search_domains("example", &crate::seta::domains::SearchPhase::Popular)
        .await
        .expect_err("a 403 challenge must fail");
    handle.await.unwrap();

    assert!(
        matches!(err, SetaError::Challenge { status: 403 }),
        "a challenge must survive into the error type, got: {err:?}"
    );
    // And it must never be retried: an unsolvable interstitial only
    // deepens the edge's reputation signal against the user.
    assert!(!err.is_transient(), "a challenge is never worth retrying");
}

// ── Invite-gate `not_allowlisted` 403 (POST /api/sites) ────────────────────
// The seta invite gate returns `403 {error:'not_allowlisted', apply_url:'<url>'}`
// for a verified email that isn't on the allowlist. `parse_not_allowlisted`
// is the pure body-parser `from_failed_response` routes a 403 through so the
// un-admitted user gets the friendly "青苔正在内测，申请免费试用" + apply link
// instead of an opaque error.

#[test]
fn test_parse_not_allowlisted_extracts_apply_url_on_403() {
    let body = r#"{"error":"not_allowlisted","apply_url":"https://mosspub.com/apply"}"#;
    assert_eq!(
        parse_not_allowlisted(403, body),
        Some("https://mosspub.com/apply".to_string())
    );
}

#[test]
fn test_parse_not_allowlisted_empty_apply_url_is_allowed() {
    // Server returns an empty apply_url string when APPLY_URL is unset.
    let body = r#"{"error":"not_allowlisted","apply_url":""}"#;
    assert_eq!(parse_not_allowlisted(403, body), Some(String::new()));
}

#[test]
fn test_parse_not_allowlisted_ignores_non_403() {
    // The `error` marker only counts on a 403 — a stray body on another
    // status must not be hijacked into the invite-gate branch.
    let body = r#"{"error":"not_allowlisted","apply_url":"https://x"}"#;
    assert_eq!(parse_not_allowlisted(400, body), None);
}

#[test]
fn test_parse_not_allowlisted_ignores_other_403_errors() {
    // A different 403 reason (e.g. the pre-existing "pubkey not verified")
    // must fall through to the normal failure path, not the invite branch.
    let body = r#"{"error":"pubkey not verified for this email"}"#;
    assert_eq!(parse_not_allowlisted(403, body), None);
}

#[test]
fn test_not_allowlisted_error_display_carries_apply_url() {
    // The variant's Display feeds the deploy-error string; it must carry the
    // apply_url so the call-site message can surface the link.
    let err = SetaError::NotAllowlisted {
        apply_url: "https://mosspub.com/apply".to_string(),
    };
    assert!(
        err.to_string().contains("https://mosspub.com/apply"),
        "Display must carry apply_url: {err}"
    );
}

#[test]
fn test_extract_api_error_extracts_json_error_field() {
    let body = r#"{"error":"Invalid domain name"}"#;
    assert_eq!(extract_api_error_message(body), "Invalid domain name");
}

#[test]
fn test_extract_api_error_falls_back_to_raw_on_non_json() {
    let body = "Service Unavailable";
    assert_eq!(extract_api_error_message(body), "Service Unavailable");
}

#[test]
fn test_extract_api_error_falls_back_when_json_lacks_error_field() {
    let body = r#"{"status":"failed"}"#;
    assert_eq!(extract_api_error_message(body), body);
}

#[test]
fn test_extract_api_error_handles_empty_body() {
    assert_eq!(extract_api_error_message(""), "");
}

#[test]
fn test_extract_api_error_extracts_message_field() {
    // Auth endpoints sometimes return `{"message": "..."}` instead of
    // `{"error": ...}`; both must be surfaced (preserves the old auth-flow
    // extraction that request_auth/request_reverify did inline).
    let body = r#"{"message":"Invalid verification code"}"#;
    assert_eq!(extract_api_error_message(body), "Invalid verification code");
}

#[test]
fn test_client_creation_uses_env_var() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let identity = test_identity();
    std::env::set_var("MOSS_SETA_URL", "https://moss-seta-test.fly.dev");
    let client = MossSetaClient::with_identity(&identity);
    assert_eq!(client.base_url, "https://moss-seta-test.fly.dev");
    std::env::remove_var("MOSS_SETA_URL");
}

#[test]
fn test_client_falls_back_to_default_when_env_not_set() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let identity = test_identity();
    std::env::remove_var("MOSS_SETA_URL");
    let client = MossSetaClient::with_identity(&identity);
    assert_eq!(client.base_url, DEFAULT_SETA_URL);
}

#[test]
fn test_custom_url_overrides_env_var() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let identity = test_identity();
    std::env::set_var("MOSS_SETA_URL", "https://env-url.example.com");
    let client = MossSetaClient::with_identity_and_url(&identity, "http://localhost:8787/");
    assert_eq!(client.base_url, "http://localhost:8787");
    std::env::remove_var("MOSS_SETA_URL");
}

#[test]
fn test_get_seta_url_from_env() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("MOSS_SETA_URL", "https://custom.example.com");
    assert_eq!(get_seta_url(), "https://custom.example.com");
    std::env::remove_var("MOSS_SETA_URL");
    assert_eq!(get_seta_url(), DEFAULT_SETA_URL);
}

#[test]
fn test_client_stores_identity() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let identity = test_identity();
    std::env::remove_var("MOSS_SETA_URL");
    let client = MossSetaClient::with_identity(&identity);
    let stored_identity = client
        .identity
        .as_ref()
        .expect("Identity should be present");
    assert_eq!(stored_identity.pubkey, identity.pubkey);
}

#[test]
fn test_sign_request_payload() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let identity = test_identity();
    std::env::remove_var("MOSS_SETA_URL");
    let client = MossSetaClient::with_identity(&identity);
    let result = client.sign_request_payload("/api/test");
    assert!(result.is_ok(), "Should be able to sign request");
    let auth_header = result.unwrap();
    assert!(
        auth_header.starts_with("Nostr "),
        "Auth header should start with 'Nostr '"
    );
}

#[tokio::test]
async fn test_seta_url_is_reachable() {
    let url = {
        let _env = crate::ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::var(SETA_URL_ENV_VAR).ok()
    };

    let url = match url {
        Some(u) if !u.contains("example.com") && !u.contains("custom.") => u,
        _ => {
            eprintln!("Skipping test_seta_url_is_reachable: MOSS_SETA_URL not set (or set to a test value)");
            return;
        }
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client");

    let response = client.get(&url).send().await;
    assert!(
        response.is_ok(),
        "Seta URL {} is not reachable: {:?}",
        url,
        response.err()
    );

    let status = response.unwrap().status();
    assert!(
        status.is_success() || status.as_u16() == 404,
        "Seta URL {} returned unexpected status: {}",
        url,
        status
    );
}

#[test]
fn test_configure_dns_url_construction() {
    let identity = test_identity();
    let client = MossSetaClient::with_identity_and_url(&identity, "https://test.example.com");
    let result = client.sign_request_payload("/api/domains/example.com/dns");
    assert!(result.is_ok(), "Should be able to sign DNS config request");
}

#[test]
fn test_new_creates_unauthenticated_client() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("MOSS_SETA_URL");
    let client = MossSetaClient::new();
    assert_eq!(client.base_url, DEFAULT_SETA_URL);
    assert!(
        client.identity.is_none(),
        "Unauthenticated client should have no identity"
    );
}

#[test]
fn test_new_uses_env_var() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("MOSS_SETA_URL", "https://moss-seta-test.fly.dev");
    let client = MossSetaClient::new();
    assert_eq!(client.base_url, "https://moss-seta-test.fly.dev");
    assert!(client.identity.is_none());
    std::env::remove_var("MOSS_SETA_URL");
}

#[test]
fn test_with_url_creates_unauthenticated_client() {
    let client = MossSetaClient::with_url("https://test.example.com");
    assert_eq!(client.base_url, "https://test.example.com");
    assert!(
        client.identity.is_none(),
        "with_url client should have no identity"
    );
}

#[test]
fn test_with_url_allows_localhost_http() {
    let client = MossSetaClient::with_url("http://localhost:8787/");
    assert_eq!(client.base_url, "http://localhost:8787");
    assert!(client.identity.is_none());
}

#[test]
#[should_panic(expected = "Security error")]
fn test_with_url_rejects_non_localhost_http() {
    MossSetaClient::with_url("http://insecure.example.com");
}

#[test]
fn test_unauthenticated_client_sign_request_fails() {
    let client = MossSetaClient::with_url("https://test.example.com");
    let result = client.sign_request_payload("/api/test");
    assert!(
        result.is_err(),
        "Unauthenticated client should not be able to sign requests"
    );
    let err = result.unwrap_err();
    match err {
        SetaError::Identity(msg) => {
            assert!(
                msg.contains("without identity"),
                "Error should mention missing identity: {}",
                msg
            );
        }
        _ => panic!("Expected Identity error, got: {:?}", err),
    }
}

#[test]
fn test_unauthenticated_client_secure_url() {
    let client = MossSetaClient::with_url("https://test.example.com");
    let url = client.secure_url("/api/domains/search");
    assert_eq!(url, "https://test.example.com/api/domains/search");
}

#[test]
fn test_with_identity_has_identity() {
    let identity = test_identity();
    let client = MossSetaClient::with_identity_and_url(&identity, "https://test.example.com");
    assert!(
        client.identity.is_some(),
        "Authenticated client should have identity"
    );
    assert_eq!(client.identity.as_ref().unwrap().pubkey, identity.pubkey);
}

#[test]
fn test_client_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MossSetaClient>();
}

fn extract_payload_from_auth_header(header: &str) -> String {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    let b64 = header
        .strip_prefix("Nostr ")
        .expect("auth header must start with 'Nostr '");
    let json = BASE64
        .decode(b64)
        .expect("auth header must be valid base64");
    let value: serde_json::Value =
        serde_json::from_slice(&json).expect("auth header must be valid JSON");
    value
        .get("payload")
        .and_then(|v| v.as_str())
        .expect("signed request must have a 'payload' field")
        .to_string()
}

proptest::proptest! {
    #[test]
    #[ignore = "blocked on dot-segment normalization"]
    fn sign_and_build_signs_what_it_sends(segment in "[^\n]{1,64}") {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("MOSS_SETA_URL");
        let identity = test_identity();
        let client = MossSetaClient::with_identity_and_url(&identity, "https://test.example.com");
        let path = format!("/api/sites/abc/files/{}", urlencoding::encode(&segment));
        let (url, auth) = client.sign_and_build(&path).expect("sign_and_build must succeed for authenticated client");
        let signed_payload = extract_payload_from_auth_header(&auth);
        let parsed = reqwest::Url::parse(&url).expect("sign_and_build must produce a parseable URL");
        let wire_path_and_query = match parsed.query() {
            Some(q) => format!("{}?{}", parsed.path(), q),
            None => parsed.path().to_string(),
        };
        proptest::prop_assert_eq!(signed_payload, wire_path_and_query);
    }
}

#[tokio::test]
async fn test_request_reverify_sends_mode_and_site_id() {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let captured: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
    let captured_bg = Arc::clone(&captured);

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        *captured_bg.lock().unwrap() = Some(request);
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nContent-Type: application/json\r\n\r\n{\"success\":true}\n";
        stream.write_all(resp).await.unwrap();
        stream.shutdown().await.ok();
    });

    let url = format!("http://{}", addr);
    let client = MossSetaClient::with_url(&url);
    client
        .request_reverify("user@example.com", "abcd1234", "sess-xyz", "my-site")
        .await
        .expect("reverify request should succeed");
    handle.await.unwrap();

    let req = captured
        .lock()
        .unwrap()
        .clone()
        .expect("server captured request");
    assert!(req.starts_with("POST /auth/request"), "path: {}", req);
    assert!(req.contains("\"mode\":\"re_verify\""), "payload: {}", req);
    assert!(req.contains("\"site_id\":\"my-site\""), "payload: {}", req);
    assert!(
        req.contains("\"email\":\"user@example.com\""),
        "payload: {}",
        req
    );
    assert!(req.contains("\"pubkey\":\"abcd1234\""), "payload: {}", req);
    assert!(
        req.contains("\"session_id\":\"sess-xyz\""),
        "payload: {}",
        req
    );
}

#[tokio::test]
async fn test_request_reverify_forwards_caller_supplied_email() {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let captured: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
    let captured_bg = Arc::clone(&captured);

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        *captured_bg.lock().unwrap() = Some(String::from_utf8_lossy(&buf[..n]).to_string());
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nContent-Type: application/json\r\n\r\n{\"success\":true}\n";
        stream.write_all(resp).await.unwrap();
        stream.shutdown().await.ok();
    });

    let url = format!("http://{}", addr);
    let client = MossSetaClient::with_url(&url);
    client
        .request_reverify("caller-supplied@example.com", "pk", "sid", "site")
        .await
        .expect("reverify ok");
    handle.await.unwrap();
    let req = captured.lock().unwrap().clone().unwrap();
    assert!(
        req.contains("\"email\":\"caller-supplied@example.com\""),
        "email arg must reach payload: {}",
        req
    );
}

/// Every request through the shared seta client must carry moss's
/// User-Agent. reqwest's default is NO User-Agent header at all — the exact
/// signal that made Cloudflare Bot Fight Mode issue managed challenges
/// against moss's auth calls in the 2026-07-21 incident (a native client
/// can never solve one, so the block was permanent). This exercises
/// `build_seta_http_client` via a real request against a local server, so
/// a reverted `.user_agent(..)` line fails here, not in production.
#[tokio::test]
async fn test_shared_client_sends_moss_user_agent() {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let captured: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
    let captured_bg = Arc::clone(&captured);

    let listener = TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        *captured_bg.lock().unwrap() = Some(String::from_utf8_lossy(&buf[..n]).to_string());
        let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nContent-Type: application/json\r\n\r\n{\"success\":true}\n";
        stream.write_all(resp).await.unwrap();
        stream.shutdown().await.ok();
    });

    let url = format!("http://{}", addr);
    let client = MossSetaClient::with_url(&url);
    client
        .request_reverify("ua@example.com", "pk", "sid", "site")
        .await
        .expect("request ok");
    handle.await.unwrap();

    let req = captured
        .lock()
        .unwrap()
        .clone()
        .expect("server captured request");
    let ua_line = req
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("user-agent:"))
        .unwrap_or_else(|| panic!("no User-Agent header in request:\n{req}"))
        .to_string();
    let ua_value = ua_line.splitn(2, ':').nth(1).unwrap().trim().to_string();
    // Assert against the SHARED constant, so this test and the net-diagnosis
    // probe's test pin the same string and neither client can drift.
    assert_eq!(
        ua_value,
        crate::system::user_agent(),
        "the seta client must send the shared moss User-Agent"
    );
    // …and pin the constant's own shape, so "shared" can't degrade into
    // "shared and wrong" without a test noticing.
    assert_eq!(
        crate::system::user_agent(),
        format!("moss/{} (+https://mosspub.com)", env!("CARGO_PKG_VERSION")),
        "User-Agent must identify moss and its version"
    );
}

#[test]
fn test_whoami_response_deserializes_with_subscription() {
    let json = r#"{
            "pubkey": "abc123",
            "email": "user@example.com",
            "subscription": {
                "plan": "paid",
                "active": true,
                "daysRemaining": null,
                "storageMb": 500,
                "bandwidthMbMonth": 1000,
                "expiresAt": null
            }
        }"#;
    let resp: WhoamiResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.pubkey, "abc123");
    assert_eq!(resp.email.as_deref(), Some("user@example.com"));
    let sub = resp.subscription.expect("subscription should be present");
    assert_eq!(sub.plan, "paid");
    assert_eq!(sub.active, Some(true));
    assert!(sub.is_active());
    assert_eq!(sub.storage_mb, 500);
    assert_eq!(sub.bandwidth_mb_month, 1000);
}

#[test]
fn test_whoami_response_deserializes_without_active_field() {
    let json = r#"{
            "pubkey": "abc123",
            "email": "user@example.com",
            "subscription": {
                "id": "sub_1",
                "pubkey": "audit",
                "email": "user@example.com",
                "plan": "pro",
                "storageMb": 500,
                "bandwidthMbMonth": 1000,
                "stripeSubscriptionId": null,
                "expiresAt": null,
                "createdAt": 1700000000
            }
        }"#;
    let resp: WhoamiResponse = serde_json::from_str(json).unwrap();
    let sub = resp.subscription.expect("subscription should be present");
    assert_eq!(sub.active, None, "server omitted `active`");
    assert!(
        sub.is_active(),
        "lifetime (expiresAt=null) must be active via fallback"
    );
}

#[test]
fn test_whoami_response_deserializes_without_active_field_expired() {
    let json = r#"{
            "pubkey": "abc123",
            "email": "user@example.com",
            "subscription": {
                "plan": "monthly",
                "storageMb": 500,
                "bandwidthMbMonth": 1000,
                "expiresAt": 100
            }
        }"#;
    let resp: WhoamiResponse = serde_json::from_str(json).unwrap();
    let sub = resp.subscription.unwrap();
    assert_eq!(sub.active, None);
    assert!(
        !sub.is_active(),
        "expired timestamp must be inactive via fallback"
    );
}

#[test]
fn test_whoami_response_deserializes_with_null_subscription() {
    let json = r#"{"pubkey": "abc123", "email": "user@example.com", "subscription": null}"#;
    let resp: WhoamiResponse = serde_json::from_str(json).unwrap();
    assert!(
        resp.subscription.is_none(),
        "null subscription should deserialize to None"
    );
}

#[test]
fn test_whoami_response_deserializes_without_subscription_field() {
    let json = r#"{"pubkey": "abc123", "email": "user@example.com"}"#;
    let resp: WhoamiResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.pubkey, "abc123");
    assert_eq!(resp.email.as_deref(), Some("user@example.com"));
    assert!(resp.subscription.is_none());
}

#[test]
fn test_whoami_response_deserializes_with_null_email_and_no_subscription() {
    let json = r#"{"pubkey": "abc123", "email": null}"#;
    let resp: WhoamiResponse = serde_json::from_str(json).unwrap();
    assert!(resp.email.is_none());
    assert!(resp.subscription.is_none());
}

#[test]
fn test_for_environment_routes_staging() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("MOSS_SETA_URL");
    let identity = test_identity();
    let client = MossSetaClient::for_environment(
        &identity,
        &crate::config::environment::HostingEnvironment::Staging,
    );
    assert_eq!(client.base_url, "https://staging.mosspub.com");
}

/// `for_environment(Production)` must resolve to the prod API URL when
/// MOSS_SETA_URL is not set. This is the regression guard for the
/// env-aware seta routing fix (B1–B7): call sites that previously always
/// called `with_identity` (→ get_seta_url → prod) must produce the same
/// result on prod, and the correct staging URL under staging.
#[test]
fn test_for_environment_routes_production() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("MOSS_SETA_URL");
    let identity = test_identity();
    let client = MossSetaClient::for_environment(
        &identity,
        &crate::config::environment::HostingEnvironment::Production,
    );
    assert_eq!(client.base_url, "https://api.mosspub.com");
}

/// `new_for_environment` (unauthenticated) must honor the environment.
/// Guards the check_site_id_available fix (check_site_id_available fix):
/// staging users querying ID availability must hit the staging server.
#[test]
fn test_new_for_environment_routes_staging() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("MOSS_SETA_URL");
    let client =
        MossSetaClient::new_for_environment(&crate::config::environment::HostingEnvironment::Staging);
    assert_eq!(client.base_url, "https://staging.mosspub.com");
    assert!(
        client.identity.is_none(),
        "new_for_environment must be unauthenticated"
    );
}

/// MOSS_SETA_URL overrides both `for_environment` and `new_for_environment`
/// regardless of the env arg — local dev override takes precedence.
#[test]
fn test_env_var_overrides_for_environment() {
    let _env = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("MOSS_SETA_URL", "https://local-seta.example.com");
    let identity = test_identity();
    let client = MossSetaClient::for_environment(
        &identity,
        &crate::config::environment::HostingEnvironment::Staging,
    );
    assert_eq!(
        client.base_url, "https://local-seta.example.com",
        "MOSS_SETA_URL must override the environment seta_url"
    );
    std::env::remove_var("MOSS_SETA_URL");
}

// The classification table itself lives in `failure_class_tests.rs`, next to
// the classifier. What is pinned here is the retry ENGINE's use of it.

/// An ordinary origin fault: retryable, and counted against the file's
/// allowance. Deliberately 502 and not 503 — 503 is `Backpressure`, which the
/// engine forgives, so using it here would test a different rule.
fn blip() -> SetaError {
    SetaError::Api {
        status: 502,
        message: "origin restarting".into(),
    }
}

/// The server asking for time rather than reporting a fault.
fn pushback() -> SetaError {
    SetaError::Api {
        status: 503,
        message: "at capacity".into(),
    }
}

/// The retry engine must never let ONE file outlive the deploy budget.
///
/// A stalled connection fails only when its own request timeout fires
/// (`upload_policy::UPLOAD_REQUEST_TIMEOUT`) and that failure is
/// transient-classed, so an attempt-counting-only policy would spend
/// `RETRY_MAX_ATTEMPTS x MAX_ATTEMPT_DURATION` on a single file. Pin the
/// wall-clock ceiling, not the attempt count.
///
/// Re-derived 2026-08-03 when the request timeout dropped 300s -> 150s (it now
/// sits just above Cloudflare's 125s cut instead of far beyond it). That
/// *loosens* this gate — four 150s attempts fit in a 600s budget where two
/// 300s attempts did not — so the old `attempts == 1` assertion was an artifact
/// of the old constants, not the invariant. The invariant is that the wall
/// clock stays inside the budget the file was given.
#[tokio::test(start_paused = true)]
async fn a_stalled_attempt_cannot_outlive_the_upload_budget() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let attempts = AtomicU32::new(0);
    let attempts_ref = &attempts;
    let started = tokio::time::Instant::now();
    let budget = RetryBudget::new();

    let result: Result<(), SetaError> =
        retry_transient("stalled.jpg", &budget, move || async move {
            attempts_ref.fetch_add(1, Ordering::SeqCst);
            // Model a connection that goes silent: the attempt burns its
            // full per-request ceiling, then surfaces as transient.
            tokio::time::sleep(MAX_ATTEMPT_DURATION).await;
            Err(blip())
        })
        .await;

    assert!(result.is_err(), "a stalled upload must still fail");
    assert!(
        started.elapsed() <= UPLOAD_RETRY_BUDGET,
        "one file spent {:?} on retries, over the {:?} budget",
        started.elapsed(),
        UPLOAD_RETRY_BUDGET
    );
    let n = attempts.load(Ordering::SeqCst);
    assert!(
        n >= 1 && n <= RETRY_MAX_ATTEMPTS,
        "attempt count {n} outside 1..={RETRY_MAX_ATTEMPTS}"
    );
    // Derived from the constants rather than hard-coded, so a future change to
    // either one re-derives instead of silently drifting: every attempt that
    // ran had to fit, worst case, inside the shared budget.
    assert!(
        MAX_ATTEMPT_DURATION * n <= UPLOAD_RETRY_BUDGET,
        "{n} attempts x {:?} exceeds the {:?} budget",
        MAX_ATTEMPT_DURATION,
        UPLOAD_RETRY_BUDGET
    );
}

/// The budget must not over-correct: a fast-failing 5xx is the exact case
/// retrying exists for, and it costs milliseconds, so all four tries run.
#[tokio::test(start_paused = true)]
async fn a_fast_transient_failure_still_gets_every_attempt() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let attempts = AtomicU32::new(0);
    let attempts_ref = &attempts;
    let budget = RetryBudget::new();

    let result: Result<(), SetaError> =
        retry_transient("index.html", &budget, move || async move {
            attempts_ref.fetch_add(1, Ordering::SeqCst);
            Err(blip())
        })
        .await;

    assert!(result.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        RETRY_MAX_ATTEMPTS,
        "a fast 503 must still get 1 attempt + 3 retries"
    );
}

/// A chunked upload shares ONE budget across all its chunks, so *futile* time
/// cannot compound per chunk. Once the shared budget is spent failing, later
/// chunks fail fast instead of each starting a fresh 4-attempt allowance.
#[tokio::test(start_paused = true)]
async fn a_shared_budget_stops_retries_compounding_across_chunks() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let budget = RetryBudget::new();

    // Chunk 1 fails after burning the file's headroom.
    //
    // "The headroom" is derived, not a magic number: the budget admits another
    // attempt only while `elapsed + delay + MAX_ATTEMPT_DURATION` still fits, so
    // it is exhausted once elapsed exceeds
    // `UPLOAD_RETRY_BUDGET - MAX_ATTEMPT_DURATION`.
    let headroom_exhausting = UPLOAD_RETRY_BUDGET - MAX_ATTEMPT_DURATION + Duration::from_secs(1);
    let first: Result<(), SetaError> = retry_transient("big.mp4", &budget, || async {
        tokio::time::sleep(headroom_exhausting).await;
        Err(blip())
    })
    .await;
    assert!(first.is_err());

    // Chunk 2 hits a transient blip with no budget left: fail, don't retry.
    let attempts = AtomicU32::new(0);
    let attempts_ref = &attempts;
    let second: Result<(), SetaError> = retry_transient("big.mp4", &budget, move || async move {
        attempts_ref.fetch_add(1, Ordering::SeqCst);
        Err(blip())
    })
    .await;

    assert!(second.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "the file's retry budget is shared, not renewed per chunk"
    );
}

/// **Invariant I1**: no deadline in the publish path may be a function of the
/// total amount of work.
///
/// The 600 s budget used to measure total time, so a file's own successful
/// transfer consumed the allowance meant for its failures — a 50 MB video on a
/// 50 KB/s uplink exhausted it while doing everything right (a large live site,
/// 2026-08-04). Success now restarts the stopwatch, so an arbitrarily large
/// file that keeps landing chunks keeps its full retry allowance.
#[tokio::test(start_paused = true)]
async fn honest_transfer_time_does_not_consume_the_retry_allowance() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let budget = RetryBudget::new();

    // Ten chunks, each taking most of the old whole-file budget to send. Under
    // the old rule the second one already had no headroom left.
    let per_chunk = UPLOAD_RETRY_BUDGET - MAX_ATTEMPT_DURATION + Duration::from_secs(1);
    for _ in 0..10 {
        let ok: Result<(), SetaError> = retry_transient("big.mp4", &budget, || async {
            tokio::time::sleep(per_chunk).await;
            Ok(())
        })
        .await;
        assert!(ok.is_ok());
    }

    // The eleventh chunk hits a blip. It must still get its full allowance:
    // nothing has gone wrong yet.
    let attempts = AtomicU32::new(0);
    let attempts_ref = &attempts;
    let blipped: Result<(), SetaError> = retry_transient("big.mp4", &budget, move || async move {
        attempts_ref.fetch_add(1, Ordering::SeqCst);
        Err(blip())
    })
    .await;

    assert!(blipped.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        RETRY_MAX_ATTEMPTS,
        "a file that is making progress must not lose its retries to its own size"
    );
}

/// The termination proof for the change above.
///
/// `note_progress` removes the only unconditional stop a single file had. The
/// chunk loop re-enters `retry_transient` per chunk AND on every `escalate_down`
/// retry, each entry granting a fresh `RETRY_MAX_ATTEMPTS`; the deploy's stall
/// watchdog bumps on each attempt boundary. So a file that alternates "one
/// chunk lands, then a round of attempts fails" satisfies the stopwatch and the
/// watchdog simultaneously and would run forever on the time bound alone.
/// `MAX_FAILED_ATTEMPTS_PER_FILE` — which progress does NOT reset — is what
/// still terminates it.
#[tokio::test(start_paused = true)]
async fn alternating_progress_and_failure_terminates() {
    let budget = RetryBudget::new();

    let mut rounds = 0u32;
    while budget.allows_another_attempt(Duration::ZERO) {
        // A chunk lands: the futile-time stopwatch restarts, and the stall
        // watchdog would see progress.
        let ok: Result<(), SetaError> =
            retry_transient("flaky.mp4", &budget, || async { Ok(()) }).await;
        assert!(ok.is_ok());

        // Then a round of failures against the next chunk.
        let bad: Result<(), SetaError> =
            retry_transient("flaky.mp4", &budget, || async { Err(blip()) }).await;
        assert!(bad.is_err());

        rounds += 1;
        assert!(
            rounds <= MAX_FAILED_ATTEMPTS_PER_FILE,
            "the file never gave up: {rounds} make-progress/fail rounds and counting"
        );
    }

    // Terminated — and not so eagerly that a couple of blips end a publish.
    assert!(
        rounds >= 2,
        "one bad round must not abandon a file that is still moving bytes"
    );
}

/// Deciding to retry is liveness, and the deploy's stall watchdog has to hear
/// it. Byte credits alone leave a window of small files that each time out
/// twice legitimately silent for 302 s — just past `activity::STALL_TIMEOUT` —
/// so a healthy publish would be killed. The counter is process-global and
/// other tests bump it too, hence the monotonic assertion rather than a count.
#[tokio::test(start_paused = true)]
async fn a_retry_reports_liveness_to_the_stall_watchdog() {
    let before = crate::infra::liveness::bump_count();
    let budget = RetryBudget::new();
    let _: Result<(), SetaError> =
        retry_transient("index.html", &budget, || async { Err(blip()) }).await;
    assert!(
        crate::infra::liveness::bump_count() > before,
        "a retrying upload must bump the activity clock, or the watchdog sees a stall"
    );
}

/// Backpressure (429/503) is the peer's admission control, not the file's
/// fault: the first few cost no retry slot, so a busy server cannot spend a
/// file's whole allowance on its own capacity problem. The grace is bounded, so
/// the attempts still stop.
#[tokio::test(start_paused = true)]
async fn backpressure_is_forgiven_a_bounded_number_of_times() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let attempts = AtomicU32::new(0);
    let attempts_ref = &attempts;
    let budget = RetryBudget::new();

    let result: Result<(), SetaError> = retry_transient("index.html", &budget, move || async move {
        attempts_ref.fetch_add(1, Ordering::SeqCst);
        Err(pushback())
    })
    .await;

    assert!(result.is_err(), "sustained backpressure must still fail");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        BACKPRESSURE_GRACE + RETRY_MAX_ATTEMPTS,
        "the forgiven attempts come on TOP of the ordinary allowance, and only \
         BACKPRESSURE_GRACE of them"
    );
}

/// …and it must wait meaningfully longer than an ordinary retry, or "forgiven"
/// just means "hammered the struggling server three extra times".
#[tokio::test(start_paused = true)]
async fn backpressure_backs_off_further_than_an_ordinary_retry() {
    let started = tokio::time::Instant::now();
    let budget = RetryBudget::new();
    let _: Result<(), SetaError> =
        retry_transient("index.html", &budget, || async { Err(pushback()) }).await;
    // Three forgiven waits with floors of 2s, 4s, 8s = 14s minimum, against a
    // reachable 3.5s ceiling for the whole ordinary-retry sequence.
    let floor = Duration::from_millis(
        BACKPRESSURE_BASE_DELAY_MS + 2 * BACKPRESSURE_BASE_DELAY_MS + 4 * BACKPRESSURE_BASE_DELAY_MS,
    );
    assert!(
        started.elapsed() >= floor,
        "backed off {:?}, expected at least {:?}",
        started.elapsed(),
        floor
    );
}

/// The backoff ceiling that is actually reachable, pinned so nobody
/// re-introduces a documented cap that never engages: with
/// `RETRY_MAX_ATTEMPTS = 4` the delays are drawn from [0,500], [0,1000],
/// [0,2000] — a cumulative worst case of 3.5s, negligible against the
/// wall-clock budget that does the real bounding.
#[test]
fn reachable_backoff_delays_are_bounded_by_the_attempt_count() {
    let mut cumulative_ceiling = 0u64;
    for retry_index in 0..RETRY_MAX_ATTEMPTS - 1 {
        let ceiling = RETRY_BASE_DELAY_MS << retry_index;
        assert!(
            full_jitter_delay(retry_index) <= Duration::from_millis(ceiling),
            "retry {} drew outside its jitter window",
            retry_index
        );
        cumulative_ceiling += ceiling;
    }
    assert_eq!(
        cumulative_ceiling, 3_500,
        "the reachable backoff total is 500+1000+2000ms; a higher documented \
             cap would be dead code"
    );
}
