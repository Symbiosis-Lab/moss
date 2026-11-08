use super::*;

#[tokio::test]
async fn sessions_slide_and_recent_admission_does_not_write() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let mut records: Value = serde_json::from_slice(&std::fs::read(&f.session_path).unwrap()).unwrap();
    records[0]["created"] = json!(secrets::now() - sessions::SESSION_SECONDS - 1);
    records[0]["last_seen"] = json!(secrets::now() - 301);
    std::fs::write(&f.session_path, serde_json::to_vec(&records).unwrap()).unwrap();
    assert_eq!(f.probe(&cookie).await, StatusCode::NOT_FOUND);
    let fresh = std::fs::read(&f.session_path).unwrap();
    let updated: Value = serde_json::from_slice(&fresh).unwrap();
    assert!(updated[0]["last_seen"].as_u64().unwrap() >= secrets::now() - 1);
    // Compact the JSON: even a same-second rewrite would change these bytes.
    let compact = serde_json::to_vec(&updated).unwrap();
    std::fs::write(&f.session_path, &compact).unwrap();
    let mut request = f.request("POST", PROBE, json!({}));
    request.headers_mut().insert("cookie", cookie.parse().unwrap());
    let response = f.send(request).await;
    assert!(response.headers()["set-cookie"].to_str().unwrap().contains("Max-Age=2592000"));
    assert_eq!(std::fs::read(&f.session_path).unwrap(), compact);
}

#[tokio::test]
async fn ui_mint_requires_strict_cookie_admission_and_returns_public_link() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let path = "/__moss/sign-in/new";
    assert_eq!(f.send(f.request("POST", path, json!({}))).await.status(), StatusCode::UNAUTHORIZED);
    for (origin, fetch, expected) in [
        (ORIGIN, "same-origin", StatusCode::OK),
        ("http://localhost:62000", "same-origin", StatusCode::FORBIDDEN),
        ("http://localhost:62002", "same-origin", StatusCode::FORBIDDEN),
        (ORIGIN, "same-site", StatusCode::FORBIDDEN),
        (ORIGIN, "cross-site", StatusCode::FORBIDDEN),
    ] {
        let mut request = f.request("POST", path, json!({}));
        request.headers_mut().insert("cookie", cookie.parse().unwrap());
        request.headers_mut().insert("origin", origin.parse().unwrap());
        request.headers_mut().insert("sec-fetch-site", fetch.parse().unwrap());
        let response = f.send(request).await;
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let code = value["url"].as_str().unwrap().strip_prefix("https://editor.example.test/__moss/sign-in#code=").unwrap();
            assert_eq!(f.exchange(code).await.status(), StatusCode::NO_CONTENT);
        }
    }
}

#[tokio::test]
async fn token_mint_refuses_cookie_and_uses_same_live_auth() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let ctx = super::super::super::InvokeCtx::standalone();
    let dir = Arc::new(std::sync::RwLock::new(f.site.clone()));
    let token = ctx.bind(&dir).unwrap().token().to_string();
    let mut server = super::super::super::ServeConfig::new(dir, 62000);
    server.invoke = Some(ctx);
    server.ui = Some(UiOriginConfig {
        port: UiPort::Fixed(62001), auth: f.auth.clone(), public_url: None,
        site_root: f.site.clone(), assets: &[],
    });
    let app = super::super::routes::preview_mint_router(&server, Some(62001)).unwrap();
    let path = "/__moss/sign-in/new";
    for credential in [None, Some(("cookie", cookie.as_str())), Some(("x-moss-token", "wrong")), Some(("x-moss-token", token.as_str()))] {
        let mut request = f.request("POST", path, json!({}));
        if let Some((name, value)) = credential { request.headers_mut().insert(name, value.parse().unwrap()); }
        let response = app.clone().oneshot(request).await.unwrap();
        if credential == Some(("x-moss-token", token.as_str())) {
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let code = value["url"].as_str().unwrap().strip_prefix(&format!("{ORIGIN}/__moss/sign-in#code=")).unwrap();
            assert_eq!(f.exchange(code).await.status(), StatusCode::NO_CONTENT);
        } else { assert_eq!(response.status(), StatusCode::UNAUTHORIZED); }
    }
}

#[tokio::test]
async fn ui_responses_forbid_framing_including_errors() {
    let f = Fixture::new();
    for path in ["/", "/bundle.js", "/missing", "/__moss/sign-in", "/__moss/sign-in/new"] {
        let response = f.send(f.request("GET", path, json!(null))).await;
        assert!(response.headers().get_all("content-security-policy").iter()
            .any(|v| v.to_str().unwrap().contains("frame-ancestors 'none'")), "{path}");
    }
    let mut request = f.request("GET", "/", json!(null));
    request.headers_mut().insert("host", "evil.test".parse().unwrap());
    assert_eq!(f.send(request).await.headers()["content-security-policy"], "frame-ancestors 'none'");
}

#[tokio::test(start_paused = true)]
async fn revoked_signed_out_and_expired_streams_end_while_idle() {
    use http_body_util::BodyExt;
    for action in ["revoke", "sign-out", "expire"] {
        let f = Fixture::new();
        let (id, cookie) = f.enrolled().await;
        let mut request = f.request("GET", "/__moss/events", json!(null));
        request.headers_mut().insert("cookie", cookie.parse().unwrap());
        let response = f.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        // Poll once to establish the idle wait before revocation.
        assert!(tokio::time::timeout(std::time::Duration::from_secs(1), body.frame()).await.is_err());
        let mut idle: Value = serde_json::from_slice(&std::fs::read(&f.session_path).unwrap()).unwrap();
        idle[0]["last_seen"] = json!(secrets::now() - 301);
        let idle = serde_json::to_vec(&idle).unwrap();
        std::fs::write(&f.session_path, &idle).unwrap();
        // An idle heartbeat must recheck validity without renewing last_seen.
        let wait = async { while body.frame().await.is_some() {} };
        assert!(tokio::time::timeout(std::time::Duration::from_secs(31), wait).await.is_err());
        assert_eq!(std::fs::read(&f.session_path).unwrap(), idle);
        match action {
            "revoke" => SignInLink::new(f.session_path.clone(), &f.site).unwrap().revoke_session(&id).unwrap(),
            "sign-out" => { f.auth.sign_out(cookie.strip_prefix("moss_ui=").unwrap()).unwrap(); }
            _ => {
                let mut records: Value = serde_json::from_slice(&std::fs::read(&f.session_path).unwrap()).unwrap();
                records[0]["last_seen"] = json!(secrets::now() - sessions::SESSION_SECONDS);
                std::fs::write(&f.session_path, serde_json::to_vec(&records).unwrap()).unwrap();
            }
        }
        let end = async { while body.frame().await.is_some() {} };
        tokio::time::timeout(std::time::Duration::from_secs(30), end).await.expect(action);
    }
}

#[tokio::test]
async fn revoked_stream_cannot_deliver_queued_event() {
    use http_body_util::BodyExt;
    let f = Fixture::new();
    let (id, cookie) = f.enrolled().await;
    let mut request = f.request("GET", "/__moss/events", json!(null));
    request.headers_mut().insert("cookie", cookie.parse().unwrap());
    let mut body = f.send(request).await.into_body();
    assert!(tokio::time::timeout(std::time::Duration::from_millis(10), body.frame()).await.is_err());
    SignInLink::new(f.session_path.clone(), &f.site).unwrap().revoke_session(&id).unwrap();
    super::super::super::events::publish_named("revocation-test", json!({}));
    assert!(tokio::time::timeout(std::time::Duration::from_secs(1), body.frame()).await.unwrap().is_none());
}

#[tokio::test]
async fn ui_file_reads_refuse_moss_internal_files() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    for path in ["/__moss/read/parse_frontmatter", "/__moss/read/read_source_text"] {
        let mut request = f.request("POST", path, json!({"filePath": ".moss/build.nosync/http-token"}));
        request.headers_mut().insert("cookie", cookie.parse().unwrap());
        let response = f.send(request).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("own files"));
    }
}
