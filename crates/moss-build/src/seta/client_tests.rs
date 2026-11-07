use super::*;
use crate::seta::wire::device_name;

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

/// One request as a server standing in for seta received it.
#[derive(Debug, Clone)]
pub(crate) struct Received {
    pub method: String,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Received {
    /// Parse one raw HTTP/1.1 request, as a hand-rolled test server reads it.
    pub(crate) fn from_raw(raw: &[u8]) -> Self {
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("a complete request head");
        let head = String::from_utf8_lossy(&raw[..split]);
        let mut lines = head.split("\r\n");
        let mut request_line = lines.next().unwrap_or("").split(' ');
        let (method, path_and_query) = (request_line.next().unwrap_or("").to_string(), request_line.next().unwrap_or("").to_string());
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        Self { method, path_and_query, headers, body: raw[split + 4..].to_vec() }
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub(crate) fn body_json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("{} {}: body is not JSON: {e}", self.method, self.path_and_query))
    }

    /// The key that signed this request, checked as seta's middleware checks
    /// it: a v2 header whose signature covers the method, path and query, and
    /// body this server received. Panics with the reason when it does not.
    pub(crate) fn signer(&self) -> String {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
        use sha2::{Digest, Sha256};
        let at = format!("{} {}", self.method, self.path_and_query);
        let auth = self.header("authorization").unwrap_or_else(|| panic!("{at}: no Authorization header"));
        let json = BASE64.decode(auth.strip_prefix("Nostr ").unwrap_or_else(|| panic!("{at}: not a Nostr header"))).unwrap();
        let h: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(h["v"], 2, "{at}: not a v2 header");
        let timestamp = h["timestamp"].as_u64().unwrap_or_else(|| panic!("{at}: timestamp is not an integer"));
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        assert!(now.abs_diff(timestamp) <= 300, "{at}: timestamp outside seta's window");
        let nonce = h["nonce"].as_str().unwrap();
        assert!(
            (32..=64).contains(&nonce.len()) && nonce.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{at}: nonce {nonce:?}"
        );
        // `new URL()` drops an empty query: `/a?` is signed as `/a`.
        let path_and_query = match self.path_and_query.split_once('?') {
            Some((path, "")) => path,
            _ => self.path_and_query.as_str(),
        };
        // GET and HEAD have no body to seta, whatever was sent.
        let body: &[u8] = if matches!(self.method.as_str(), "GET" | "HEAD") { b"" } else { &self.body };
        let message = [
            "moss-seta-v2".to_string(),
            timestamp.to_string(),
            nonce.to_string(),
            self.method.to_ascii_uppercase(),
            path_and_query.to_string(),
            hex::encode(Sha256::digest(body)),
        ]
        .join("\n");
        let pubkey = h["pubkey"].as_str().unwrap();
        let key = k256::schnorr::VerifyingKey::from_slice(&hex::decode(pubkey).unwrap()).unwrap();
        let signature = k256::schnorr::Signature::try_from(hex::decode(h["signature"].as_str().unwrap()).unwrap().as_slice()).unwrap();
        key.verify_raw(&Sha256::digest(message.as_bytes()), &signature)
            .unwrap_or_else(|e| panic!("{at}: the signature does not cover the request as received: {e}"));
        pubkey.to_string()
    }
}

/// A local server standing in for seta: it answers every request with
/// `status`, `headers` and the body `respond` picks for its method and path,
/// and records what it received.
pub(crate) struct StandIn {
    server: mockito::ServerGuard,
    seen: std::sync::Arc<std::sync::Mutex<Vec<Received>>>,
}

impl StandIn {
    pub(crate) async fn new(
        status: usize,
        headers: &[(&'static str, &str)],
        respond: impl Fn(&str, &str) -> String + Send + Sync + 'static,
    ) -> Self {
        let mut server = mockito::Server::new_async().await;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let respond = std::sync::Arc::new(respond);
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] {
            let (log, respond) = (std::sync::Arc::clone(&seen), std::sync::Arc::clone(&respond));
            let mut mock = server
                .mock(method, mockito::Matcher::Any)
                .match_request(move |r| {
                    log.lock().unwrap().push(Received {
                        method: r.method().to_string(),
                        path_and_query: r.path_and_query().to_string(),
                        headers: r.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string())).collect(),
                        body: r.body().cloned().unwrap_or_default(),
                    });
                    true
                })
                .with_status(status)
                .with_body_from_request(move |r| respond(r.method(), r.path_and_query()).into_bytes());
            for (name, value) in headers {
                mock = mock.with_header(*name, value);
            }
            mock.create_async().await;
        }
        Self { server, seen }
    }

    /// Answers every request with `status` and the JSON `body`.
    pub(crate) async fn json(status: usize, body: &str) -> Self {
        let body = body.to_string();
        Self::new(status, &[("content-type", "application/json")], move |_, _| body.clone()).await
    }

    pub(crate) fn url(&self) -> String {
        self.server.url()
    }

    pub(crate) fn requests(&self) -> Vec<Received> {
        self.seen.lock().unwrap().clone()
    }
}

/// A Cloudflare managed-challenge response, in the shape the edge really
/// sends it.
async fn challenge() -> StandIn {
    let page = "<!DOCTYPE html><html><head><title>Just a moment...</title></head>\
            <body><script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/chl_page\"></script></body></html>";
    StandIn::new(
        403,
        &[("cf-mitigated", "challenge"), ("cf-ray", "8f0a1b2c3d4e5f60-SJC"), ("content-type", "text/html")],
        move |_, _| page.to_string(),
    )
    .await
}

/// `fetch_stripe_publishable_key` used to hand-roll its own `SetaError::Api`
/// straight from `resp.text()`, bypassing `from_failed_response`. That made
/// it the one endpoint with no cf-ray in the log, no length cap, and no
/// challenge classification — so a challenge page reached the payment UI as
/// a wall of raw HTML. Every endpoint goes through the one door.
#[tokio::test]
async fn the_stripe_key_endpoint_classifies_a_challenge_like_every_other() {
    let server = challenge().await;
    let client = MossSetaClient::with_url(&server.url());
    let err = client
        .fetch_stripe_publishable_key()
        .await
        .expect_err("a 403 challenge must fail");

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
    let server = StandIn::json(409, r#"{"error":"Domain is already linked to another site"}"#).await;
    let identity = test_identity();
    let client = MossSetaClient::with_identity_and_url(&identity, &server.url());
    let err = client
        .link_custom_domain("site", "example.com")
        .await
        .expect_err("a 409 conflict must fail");

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
    let server = challenge().await;
    let identity = test_identity();
    let client = MossSetaClient::with_identity_and_url(&identity, &server.url());
    let err = client
        .link_custom_domain("site", "example.com")
        .await
        .expect_err("a 403 challenge must fail");

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
    let server = challenge().await;
    let client = MossSetaClient::with_url(&server.url());
    let err = client
        .search_domains("example", &crate::seta::domains::SearchPhase::Popular)
        .await
        .expect_err("a 403 challenge must fail");

    assert!(
        matches!(err, SetaError::Challenge { status: 403 }),
        "a challenge must survive into the error type, got: {err:?}"
    );
    // And it must never be retried: an unsolvable interstitial only
    // deepens the edge's reputation signal against the user.
    assert!(!err.is_transient(), "a challenge is never worth retrying");
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
fn only_a_client_with_a_loaded_key_can_sign() {
    assert!(MossSetaClient::with_identity_and_url(&test_identity(), "https://test.example.com").can_sign());
    let anonymous = MossSetaClient::with_url("https://test.example.com");
    assert!(!anonymous.can_sign());
    match anonymous.signed(reqwest::Method::GET, "/api/test", bytes::Bytes::new()) {
        Err(SetaError::Identity(msg)) => assert!(msg.contains("without identity"), "{msg}"),
        other => panic!("expected an Identity error, got {:?}", other.map(|_| ())),
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

#[tokio::test]
async fn test_request_reverify_sends_mode_and_site_id() {
    let server = StandIn::json(200, r#"{"success":true}"#).await;
    MossSetaClient::with_url(&server.url())
        .request_reverify("user@example.com", "abcd1234", "sess-xyz", "my-site")
        .await
        .expect("reverify request should succeed");
    let req = &server.requests()[0];
    assert_eq!((req.method.as_str(), req.path_and_query.as_str()), ("POST", "/auth/request"));
    let body = req.body_json();
    assert_eq!(body["mode"], "re_verify");
    assert_eq!(body["site_id"], "my-site");
    assert!(body["client"].is_object(), "the confirm page names the app: {body}");
    assert_eq!(body["email"], "user@example.com");
    assert_eq!(body["pubkey"], "abcd1234");
    assert_eq!(body["session_id"], "sess-xyz");
}

#[tokio::test]
async fn test_request_reverify_forwards_caller_supplied_email() {
    let server = StandIn::json(200, r#"{"success":true}"#).await;
    MossSetaClient::with_url(&server.url())
        .request_reverify("caller-supplied@example.com", "pk", "sid", "site")
        .await
        .expect("reverify ok");
    assert_eq!(server.requests()[0].body_json()["email"], "caller-supplied@example.com");
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
    let server = StandIn::json(200, r#"{"success":true}"#).await;
    MossSetaClient::with_url(&server.url())
        .request_reverify("ua@example.com", "pk", "sid", "site")
        .await
        .expect("request ok");
    let ua_value = server.requests()[0].header("user-agent").expect("a User-Agent header").to_string();
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


#[test]
fn transport_policy_rejects_remote_http_and_credentials() {
    for address in [
        "http://localhost.attacker.invalid",
        "http://127.0.0.1.attacker.invalid",
        "http://localhost@attacker.invalid",
        "http://attacker.invalid/path?localhost",
        "http://attacker.invalid/path?127.0.0.1",
        "https://user:password@example.com",
        "http://user:password@127.0.0.1:8787",
        "ftp://localhost/file",
        "not a url containing localhost",
    ] {
        assert!(!is_safe_base_url(address), "accepted unsafe address: {address}");
        assert!(std::panic::catch_unwind(|| MossSetaClient::with_url(address)).is_err());
        let identity = test_identity();
        assert!(std::panic::catch_unwind(|| MossSetaClient::with_identity_and_url(&identity, address)).is_err());
    }
}

#[test]
fn transport_policy_preserves_https_paths_and_parsed_loopback() {
    for address in [
        "https://example.com:8443/api/v2/",
        "https://localhost.attacker.invalid/api",
        "http://localhost:8787/api/",
        "http://127.0.0.2:8787/api",
        "http://[::1]:8787/api",
        "http://[::ffff:127.0.0.1]:8787/api",
    ] {
        assert!(is_safe_base_url(address), "rejected safe address: {address}");
        assert_eq!(MossSetaClient::with_url(address).base_url, address.trim_end_matches('/'));
    }
}

#[test]
fn unsafe_runtime_override_falls_back_for_every_constructor() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var(SETA_URL_ENV_VAR);
    let fallback = get_seta_url();
    let identity = test_identity();
    let environment = crate::config::environment::HostingEnvironment::Staging;
    for address in ["http://localhost.attacker.invalid", "http://localhost@attacker.invalid", "http://attacker.invalid/?localhost"] {
        std::env::set_var(SETA_URL_ENV_VAR, address);
        let default = get_seta_url();
        let anonymous = MossSetaClient::new_for_environment(&environment).base_url;
        let authenticated = MossSetaClient::for_environment(&identity, &environment).base_url;
        std::env::remove_var(SETA_URL_ENV_VAR);
        assert_eq!(default, fallback);
        assert_eq!(anonymous, environment.seta_url());
        assert_eq!(authenticated, environment.seta_url());
    }
}

/// A stand-in that redirects every request to `location`.
async fn redirect_to(location: &str) -> StandIn {
    StandIn::new(302, &[("location", location)], |_, _| String::new()).await
}

#[tokio::test]
async fn transport_policy_blocks_redirect_credentials_before_the_destination() {
    let destination = StandIn::json(200, r#"{"status":"pending","email":null}"#).await;
    let credential_destination = destination.url().replacen("http://", "http://user:password@", 1);
    let source = redirect_to(&format!("{credential_destination}/auth/status/test-session")).await;
    let result = MossSetaClient::with_url(&source.url()).check_auth_status("test-session").await;
    match result {
        Err(SetaError::Http(error)) => assert!(error.is_redirect(), "{error}"),
        result => panic!("unsafe redirect should fail before a second request: {result:?}"),
    }
    assert!(destination.requests().is_empty(), "the redirect reached its destination");
}

#[tokio::test]
async fn transport_policy_preserves_local_redirects() {
    let destination = StandIn::json(200, r#"{"status":"pending","email":null}"#).await;
    let source = redirect_to(&format!("{}/auth/status/test-session", destination.url())).await;
    let result = MossSetaClient::with_url(&source.url()).check_auth_status("test-session").await.unwrap();
    assert_eq!(result.status, "pending");
}

#[tokio::test]
async fn transport_policy_blocks_remote_http_redirects_before_connecting() {
    let source = redirect_to("http://localhost.attacker.invalid/auth/status/test-session").await;
    let result = MossSetaClient::with_url(&source.url()).check_auth_status("test-session").await;
    match result {
        Err(SetaError::Http(error)) => assert!(error.is_redirect(), "{error}"),
        result => panic!("remote HTTP redirect should fail before connecting: {result:?}"),
    }
}

// ── `site_limit_reached` 403 ────────────────────────────────────────────────

#[test]
fn site_limit_reached_parses_with_and_without_contact() {
    let with = r#"{"error":"site_limit_reached","limit":3,"contact":"help@example.com"}"#;
    assert_eq!(
        parse_site_limit_reached(403, with),
        Some((3, Some("help@example.com".to_string())))
    );
    let without = r#"{"error":"site_limit_reached","limit":1}"#;
    assert_eq!(parse_site_limit_reached(403, without), Some((1, None)));
    let blank = r#"{"error":"site_limit_reached","limit":2,"contact":"  "}"#;
    assert_eq!(parse_site_limit_reached(403, blank), Some((2, None)));
}

#[test]
fn unknown_403_bodies_are_not_site_limit_reached() {
    assert_eq!(parse_site_limit_reached(403, r#"{"error":"forbidden"}"#), None);
    assert_eq!(parse_site_limit_reached(403, r#"{"error":"site_limit_reached"}"#), None);
    assert_eq!(parse_site_limit_reached(403, "<html>nope</html>"), None);
    assert_eq!(parse_site_limit_reached(400, r#"{"error":"site_limit_reached","limit":1}"#), None);
}

// ── `lang` on the verification request ─────────────────────────────────────

#[test]
fn auth_request_body_sends_lang_only_when_valid() {
    let body = |l| auth_request_body("a@example.com", "pk", "sid", l);
    assert!(body(None).get("lang").is_none());
    assert_eq!(body(Some("zh-hans"))["lang"], "zh-hans");
    assert_eq!(body(Some("EN"))["lang"], "EN");
    assert_eq!(body(Some("pt-br-x1"))["lang"], "pt-br-x1");
    for bad in ["", "e", "english", "en_US", "en-", "en-x", "en-toolongtag1", "a-b-c-d", "zh hans", "en-us-a1-b2"] {
        assert!(body(Some(bad)).get("lang").is_none(), "{bad:?} must be dropped");
    }
    assert_eq!(body(None)["email"], "a@example.com");
}

#[test]
fn auth_request_body_names_this_app_for_the_confirm_page() {
    let body = auth_request_body("a@example.com", "pk", "sid", None);
    let name = body["client"]["name"].as_str().unwrap();
    assert_eq!(name, device_name("moss", crate::system::device_kind()));
    assert_eq!(body["client"]["version"], crate::system::app_version());
    let os = body["client"]["os"].as_str().unwrap();
    assert!(!os.is_empty() && os != "macos", "a person's name for the OS, got {os:?}");
}

#[tokio::test]
async fn a_site_limit_403_through_the_failed_response_path_is_site_limit_reached() {
    let server = StandIn::json(403, r#"{"error":"site_limit_reached","limit":2,"contact":"help@example.com"}"#).await;
    let err = MossSetaClient::with_url(&server.url())
        .request_auth("user@example.com", "pk", "sid", None)
        .await
        .expect_err("a 403 is an error");
    assert!(
        matches!(&err, SetaError::SiteLimitReached { limit: 2, contact: Some(c) } if c == "help@example.com"),
        "{err:?}"
    );
}

/// The refusal a sign-in link lifts is logged at info, so the command line
/// shows only the "Sent a sign-in link" line and counts no problem; any other
/// 403 stays a warning. The bodies carry a token of their own: the captured
/// log is shared by the whole test process.
#[tokio::test]
async fn a_refusal_to_verify_the_email_is_no_warning() {
    use crate::build::cache::tests::logged_count;
    logged_count(log::Level::Info, "");
    let server = StandIn::json(403, r#"{"error":"Email verification required (refusal-4b1e)"}"#).await;
    let err = MossSetaClient::with_url(&server.url()).request_auth("user@example.com", "pk", "sid", None).await.expect_err("a 403 is an error");
    assert!(matches!(&err, SetaError::Api { status: 403, message } if asks_to_verify_email(403, message)), "{err:?}");
    assert_eq!(logged_count(log::Level::Warn, "refusal-4b1e"), 0);
    assert_eq!(logged_count(log::Level::Info, "refusal-4b1e"), 1);

    let server = StandIn::json(403, r#"{"error":"Not the owner of this site (refusal-7c2d)"}"#).await;
    MossSetaClient::with_url(&server.url()).request_auth("user@example.com", "pk", "sid", None).await.expect_err("a 403 is an error");
    assert_eq!(logged_count(log::Level::Warn, "refusal-7c2d"), 1);
}

/// The body a stand-in gives each request of a signed call, so the call goes
/// as far as it would against seta.
fn seta_like(method: &str, path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    match (method, path.rsplit('/').next()) {
        ("POST", Some("upload")) => r#"{"uploadId":"u1"}"#.into(),
        ("GET", Some("uploads")) => "[]".into(),
        _ => "{}".into(),
    }
}

/// Every request `call` makes against a stand-in, each checked to be signed
/// by `identity` the way seta checks it.
pub(crate) async fn signed_requests<F, Fut, T>(identity: &Identity, call: F) -> Vec<Received>
where
    F: FnOnce(MossSetaClient) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let server = StandIn::new(200, &[("content-type", "application/json")], seta_like).await;
    let _ = call(MossSetaClient::with_identity_and_url(identity, &server.url())).await;
    let requests = server.requests();
    for request in &requests {
        assert_eq!(request.signer(), identity.pubkey, "{} {}", request.method, request.path_and_query);
    }
    requests
}

/// One test per signed call: what it sends is what it signed, on the method
/// and path and query the call is for.
macro_rules! signed_call {
    ($($name:ident: $method:literal $path:literal => |$c:ident| $call:expr;)*) => {$(
        #[tokio::test]
        async fn $name() {
            let requests = signed_requests(&test_identity(), |$c| async move { $call.await }).await;
            let first = requests.first().expect("the call sent a request");
            assert_eq!((first.method.as_str(), first.path_and_query.as_str()), ($method, $path));
        }
    )*};
}

signed_call! {
    whoami_is_signed: "GET" "/auth/whoami" => |c| c.whoami();
    request_transfer_is_signed: "POST" "/api/sites/s1/transfer" => |c| c.request_transfer("s1", "to@example.com", true, false);
    cancel_transfer_is_signed: "DELETE" "/api/sites/s1/transfer" => |c| c.cancel_transfer("s1");
    send_logs_is_signed: "POST" "/api/logs" => |c| c.send_logs(&serde_json::json!({ "lines": ["a"] }));
    get_domain_cdn_is_signed: "GET" "/api/domains/example.com/cdn" => |c| c.get_domain_cdn("example.com");
    get_domain_connect_is_signed: "GET" "/api/domains/example.com/connect" => |c| c.get_domain_connect("example.com");
    start_domain_cdn_is_signed: "POST" "/api/domains/example.com/cdn" => |c| c.start_domain_cdn("example.com");
    link_custom_domain_is_signed: "POST" "/api/sites/s1/domains" => |c| c.link_custom_domain("s1", "example.com");
    configure_dns_is_signed: "POST" "/api/domains/example.com/dns" => |c| c.configure_dns("example.com", &[
        serde_json::from_value(serde_json::json!({ "record_type": "A", "name": "@", "value": "192.0.2.1" })).unwrap(),
    ]);
    get_dns_zone_is_signed: "GET" "/api/domains/example.com/dns" => |c| c.get_dns_zone("example.com");
    a_non_ascii_domain_is_signed_as_sent: "GET" "/api/domains/b%C3%BCcher.de/dns" => |c| c.get_dns_zone("bücher.de");
    a_space_is_signed_as_sent: "GET" "/api/domains/a%20b/dns" => |c| c.get_dns_zone("a b");
    a_slash_in_a_domain_stays_in_its_segment: "GET" "/api/domains/a%2F..%2Fb/dns" => |c| c.get_dns_zone("a/../b");
    an_empty_query_is_signed_as_the_server_reads_it: "GET" "/auth/whoami?" => |c| c.get_json::<serde_json::Value>("/auth/whoami?");
    create_payment_intent_is_signed: "POST" "/api/domains/create-payment" => |c| c.create_payment_intent("example.com", 1);
    charge_saved_for_domain_is_signed: "POST" "/api/payment/domains/charge-saved" => |c| c.charge_saved_for_domain("pm_1", "example.com", 1);
    get_payment_method_billing_is_signed: "GET" "/api/payment/methods/pm_1/billing" => |c| c.get_payment_method_billing("pm_1");
    list_payment_methods_is_signed: "GET" "/api/payment/methods" => |c| c.list_payment_methods();
    get_domains_is_signed: "GET" "/api/domains" => |c| c.get_domains();
    register_domain_is_signed: "POST" "/api/domains/register" => |c| c.register_domain("example.com", "pi_1", &crate::seta::domains::RegistrantContact {
        name: "A B".into(), phone: "+1.5555550100".into(), address1: "1 Main St".into(), country: "US".into(), city: None,
    });
    get_assignable_domains_is_signed: "GET" "/api/domains/owned" => |c| c.get_assignable_domains();
    assign_domain_is_signed: "PUT" "/api/domains/example.com/assign" => |c| c.assign_domain("example.com", "s1");
    get_domain_status_is_signed: "GET" "/api/domains/example.com/status" => |c| c.get_domain_status_for_host("example.com");
    register_site_is_signed: "POST" "/api/sites" => |c| c.register_site("s1");
    sync_manifest_is_signed: "POST" "/api/sites/s1/sync" => |c| c.sync_manifest("s1", &[("a.html".to_string(), "h".to_string())].into(), "g1");
    upload_file_is_signed: "PUT" "/api/sites/s1/files/docs%2Fa%20b.html" => |c| c.upload_file("s1", "docs/a b.html", b"<p>".to_vec(), "g1", None);
    upload_symlink_is_signed: "PUT" "/api/sites/s1/files/latest" => |c| c.upload_symlink("s1", "latest", "v2".into(), "g1");
    list_upload_sessions_is_signed: "GET" "/api/sites/s1/uploads?generation_id=g1" => |c| c.list_upload_sessions("s1", "g1");
    commit_sync_is_signed: "POST" "/api/sites/s1/commit" => |c| c.commit_sync("s1", &[("a.html".to_string(), "h".to_string())].into(), "g1");
    live_generation_is_signed: "GET" "/api/sites/s1/generation" => |c| c.live_generation("s1");
    list_site_domains_is_signed: "GET" "/api/sites/s1/domains" => |c| c.list_site_domains("s1");
    pull_site_data_is_signed: "GET" "/api/sites/s1/data" => |c| c.pull_site_data("s1");
    pull_events_is_signed: "GET" "/api/sites/s1/events?since=5&limit=10" => |c| c.pull_events("s1", 5, 10);
}

/// A JSON body is signed as the bytes sent, so seta reads the same document.
#[tokio::test]
async fn a_json_body_is_sent_as_signed() {
    let requests = signed_requests(&test_identity(), |c| async move { c.register_site("my-site").await }).await;
    assert_eq!(requests[0].body, br#"{"id":"my-site"}"#);
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
}

#[test]
fn the_device_name_is_the_app_on_the_kind_of_computer() {
    assert_eq!(device_name("moss command line", Some("Mac Studio")), "moss command line on Mac Studio");
    assert_eq!(device_name("moss", Some("a Windows PC")), "moss on a Windows PC");
    assert_eq!(device_name("moss command line", None), "moss command line");
}

#[test]
fn a_mac_is_named_by_its_family_only() {
    use crate::system::mac_kind;
    for (model, kind) in [
        ("Mac Studio (2023)", "Mac Studio"),
        ("MacBook Air (M2, 2022)", "MacBook Air"),
        ("MacBookPro16,1", "MacBook Pro"),
        ("MacBook10,1", "MacBook"),
        ("Macmini9,1", "Mac mini"),
        ("iMac (24-inch, M1, 2021)", "iMac"),
        ("Mac14,14", "a Mac"),
        ("", "a Mac"),
        ("Ana’s Mac Studio", "a Mac"),
    ] {
        assert_eq!(mac_kind(model), kind, "{model:?}");
    }
}

/// This Mac's family is the one System Information names, so a misread of the
/// device tree cannot hide behind the "a Mac" fallback.
#[cfg(target_os = "macos")]
#[test]
fn this_mac_is_named_by_the_family_system_information_gives() {
    let out = std::process::Command::new("system_profiler").args(["SPHardwareDataType", "-json"]).output().unwrap();
    let info: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let machine = info["SPHardwareDataType"][0]["machine_name"].as_str().unwrap_or_default();
    assert_eq!(crate::system::device_kind(), Some(crate::system::mac_kind(machine)), "System Information says {machine:?}");
}

/// The name its owner gave this computer often holds the owner's own name,
/// so a sign-in request carries one of a fixed list of device kinds and
/// nothing taken from the computer's name.
#[test]
fn the_owners_computer_name_never_reaches_the_sign_in_request() {
    let body = auth_request_body("a@example.com", "pk", "sid", None);
    let sent = body["client"]["name"].as_str().unwrap().to_string();
    let allowed: Vec<String> = std::iter::once("moss".to_string())
        .chain(crate::system::DEVICE_KINDS.iter().map(|k| format!("moss on {k}")))
        .collect();
    assert!(allowed.contains(&sent), "{sent:?} is not from the fixed list");
    if cfg!(target_os = "macos") {
        for source in [["scutil", "--get", "ComputerName"], ["scutil", "--get", "LocalHostName"]] {
            let out = std::process::Command::new(source[0]).args(&source[1..]).output().unwrap();
            let set = String::from_utf8(out.stdout).unwrap().trim().to_string();
            if !set.is_empty() && !crate::system::DEVICE_KINDS.contains(&set.as_str()) {
                assert!(!sent.contains(&set), "{sent:?} carries the computer's name {set:?}");
            }
        }
    }
}

#[test]
fn an_error_with_a_detail_keeps_it() {
    let body = r#"{"error":"key_shared","detail":"this folder's key also publishes second-site"}"#;
    assert_eq!(extract_api_error_message(body), "key_shared: this folder's key also publishes second-site");
    assert_eq!(extract_api_error_message(r#"{"error":"forbidden"}"#), "forbidden");
}
