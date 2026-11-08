use super::*;

#[tokio::test]
async fn get_never_spends_code() {
    let f = Fixture::new();
    let link = f.auth.mint_sign_in().unwrap();
    assert_eq!(link.url_path(), format!("/__moss/sign-in#code={}", link.code));
    assert_eq!(URL_SAFE_NO_PAD.decode(&link.code).unwrap().len(), 32);
    for _ in 0..2 {
        let mut request = f.request("GET", link.url_path().split('#').next().unwrap(), json!(null));
        assert!(request.uri().query().is_none());
        request.headers_mut().remove("sec-fetch-site");
        request.headers_mut().remove("origin");
        let response = f.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        let bytes = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        let script = html
            .split("<script>")
            .nth(1)
            .unwrap()
            .split("</script>")
            .next()
            .unwrap();
        let hash = base64::engine::general_purpose::STANDARD.encode(ring::digest::digest(
            &ring::digest::SHA256,
            script.as_bytes(),
        ));
        assert!(csp.contains(&format!("'sha256-{hash}'")));
        assert!(script.contains("method: 'POST'"));
        assert!(script.contains("new URLSearchParams(location.hash.slice(1))"));
        assert!(script.find("history.replaceState").unwrap() < script.find("fetch(").unwrap());
        assert!(!html.contains(&link.code));
    }
    assert_eq!(
        f.exchange(&link.code).await.status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn code_works_once_and_unknown_used_expired_are_401() {
    let f = Fixture::new();
    assert_eq!(
        f.exchange("unknown").await.status(),
        StatusCode::UNAUTHORIZED
    );
    let link = f.auth.mint_sign_in().unwrap();
    assert_eq!(
        f.exchange(&link.code).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.exchange(&link.code).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let link = f.auth.mint_sign_in().unwrap();
    f.auth.expire(&link.code);
    assert_eq!(
        f.exchange(&link.code).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn code_consumption_is_atomic() {
    let f = Fixture::new();
    let code = f.auth.mint_sign_in().unwrap().code;
    let (a, b) = tokio::join!(f.exchange(&code), f.exchange(&code));
    let mut statuses = [a.status().as_u16(), b.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [204, 401]);
}

#[tokio::test]
async fn fresh_cookie_admits_and_sign_out_revokes() {
    let f = Fixture::new();
    let link = f.auth.mint_sign_in().unwrap();
    let mut request = f.request("POST", "/__moss/sign-in", json!({"code":link.code}));
    request
        .headers_mut()
        .insert("host", "editor.example.test".parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "https://editor.example.test".parse().unwrap());
    request
        .headers_mut()
        .insert("user-agent", "test browser".parse().unwrap());
    let response = f.send(request).await;
    let header = response.headers()["set-cookie"].to_str().unwrap();
    for attribute in [
        "HttpOnly",
        "SameSite=Strict",
        "Path=/",
        "Secure",
        "Max-Age=2592000",
    ] {
        assert!(header.contains(attribute));
    }
    let cookie = header.split(';').next().unwrap().to_string();
    let secret = cookie.strip_prefix("moss_ui=").unwrap();
    assert_eq!(URL_SAFE_NO_PAD.decode(secret).unwrap().len(), 32);
    assert_ne!(secret, link.code);
    assert_eq!(f.probe(&cookie).await, StatusCode::NOT_FOUND);
    let carrier = std::fs::read_to_string(f.site.join(".moss/build.nosync/http-token")).unwrap();
    assert_ne!(secret, carrier.trim());
    assert_eq!(
        f.auth.list_sessions().unwrap()[0].user_agent,
        "test browser"
    );
    let file = std::fs::read_to_string(&f.session_path).unwrap();
    assert!(!file.contains(secret));
    let mut request = f.request("POST", "/__moss/sign-out", json!(null));
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = f.send(request).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    assert_eq!(f.probe(&cookie).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn external_revocation_is_seen_on_next_request() {
    let f = Fixture::new();
    let (id, cookie) = f.enrolled().await;
    assert_eq!(f.probe(&cookie).await, StatusCode::NOT_FOUND);
    let other = SignInLink::new(f.session_path.clone(), &f.site).unwrap();
    other.revoke_session(&id).unwrap();
    assert_eq!(f.probe(&cookie).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sessions_survive_new_server_and_revoke_all() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let auth = Arc::new(SignInLink::new(f.session_path.clone(), &f.site).unwrap());
    let app = super::super::router(
        UiOriginConfig {
            port: UiPort::Fixed(62001),
            auth: auth.clone(),
            assets: &[],
            public_url: None,
            site_root: f.site.clone(),
        },
        62001,
        "http://localhost:62000".into(),
        Some(super::super::super::InvokeCtx::standalone()),
        Arc::new(std::sync::RwLock::new(f.site.clone())),
        super::super::super::HostKind::Cli,
        Arc::default(),
    )
    .unwrap();
    let request = || {
        let mut r = f.request("POST", PROBE, json!({}));
        r.headers_mut().insert("cookie", cookie.parse().unwrap());
        r
    };
    assert_eq!(
        app.clone().oneshot(request()).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let id = auth.list_sessions().unwrap()[0].id.clone();
    auth.revoke_session(&id).unwrap();
    assert_eq!(
        app.clone().oneshot(request()).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    f.enrolled().await;
    auth.revoke_all().unwrap();
    assert!(auth.list_sessions().unwrap().is_empty());
    assert_eq!(
        app.oneshot(request()).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn private_session_file_outside_site_only() {
    let f = Fixture::new();
    f.enrolled().await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&f.session_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert!(SignInLink::new(f.site.join(".moss/sessions.json"), &f.site).is_err());
    assert!(f
        .auth
        .validate_location(f.session_path.parent().unwrap())
        .is_err());
}

#[tokio::test]
async fn config_exposes_external_preview_url() {
    let f = Fixture::new();
    let response = f
        .send(f.request("GET", "/__moss/config", json!(null)))
        .await;
    let bytes = axum::body::to_bytes(response.into_body(), 10000)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["preview_url"],
        "https://preview.example.test"
    );
}

#[tokio::test]
async fn expired_session_is_refused() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let mut records: Value =
        serde_json::from_slice(&std::fs::read(&f.session_path).unwrap()).unwrap();
    records[0]["last_seen"] = json!(secrets::now() - sessions::SESSION_SECONDS);
    std::fs::write(&f.session_path, serde_json::to_vec(&records).unwrap()).unwrap();
    assert_eq!(f.probe(&cookie).await, StatusCode::UNAUTHORIZED);
}
