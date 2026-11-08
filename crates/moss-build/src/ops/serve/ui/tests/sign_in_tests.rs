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
        62000,
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

/// A browser reaches the preview the way it reached the UI: one signed in at
/// loopback has no cookie for the public origin, so it gets the loopback preview.
#[tokio::test]
async fn config_names_the_preview_at_the_origin_the_browser_used() {
    let f = &Fixture::new();
    let config = |host: &'static str, origin: &'static str| {
        let mut request = f.request("GET", "/__moss/config", json!(null));
        request.headers_mut().insert("host", host.parse().unwrap());
        request.headers_mut().insert("origin", origin.parse().unwrap());
        async move {
            let response = f.send(request).await;
            serde_json::from_slice::<Value>(&axum::body::to_bytes(response.into_body(), 10000).await.unwrap()).unwrap()
        }
    };
    let public = config("editor.example.test", "https://editor.example.test").await;
    assert_eq!(public["preview_url"], "https://editor.example.test:444");
    assert_eq!(public["public_url"], "https://editor.example.test");
    let local = config("localhost:62001", ORIGIN).await;
    assert_eq!(local["preview_url"], "http://localhost:62000");
    assert_eq!(local["public_url"], "https://editor.example.test");
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

#[tokio::test]
async fn sources_are_served_behind_the_session() {
    let f = Fixture::new();
    // allow:raw_write a fixture source in a fresh temp dir
    std::fs::write(f.site.join("photo.txt"), "the source").unwrap();
    let get_at = |path: &str, cookie: Option<&str>| {
        let mut request = f.request("GET", path, json!(null));
        if let Some(cookie) = cookie {
            request.headers_mut().insert("cookie", cookie.parse().unwrap());
        }
        f.send(request)
    };
    let get = |cookie| get_at("/__moss/source/photo.txt", cookie);
    assert_eq!(get(None).await.status(), StatusCode::UNAUTHORIZED);
    let (_, cookie) = f.enrolled().await;
    let response = get(Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    // Sandboxed on the UI origin too, under the origin's own framing policy.
    let policies: Vec<_> = response.headers().get_all("content-security-policy").iter().map(|v| v.to_str().unwrap().to_owned()).collect();
    assert!(policies.iter().any(|p| p.starts_with("sandbox; default-src 'none'")), "{policies:?}");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(&axum::body::to_bytes(response.into_body(), 1000).await.unwrap()[..], b"the source");
    // The query reaches the core, which refuses a width outside the ladder.
    assert_eq!(get_at("/__moss/source/photo.txt?w=1000", Some(&cookie)).await.status(), StatusCode::BAD_REQUEST);
}

/// A browser's PDF plugin asks for the source with no fetch metadata and no Origin.
#[tokio::test]
async fn a_source_loads_without_fetch_metadata_and_nothing_else_does() {
    let f = Fixture::new();
    // allow:raw_write a fixture source in a fresh temp dir
    std::fs::write(f.site.join("doc.pdf"), "%PDF-").unwrap();
    let mut request = f.request("GET", "/__moss/source/doc.pdf", json!(null));
    request.headers_mut().remove("origin");
    request.headers_mut().remove("sec-fetch-site");
    assert_eq!(f.send(request).await.status(), StatusCode::UNAUTHORIZED);
    let (_, cookie) = f.enrolled().await;
    let bare = |method: &str, path: &str, site: Option<&str>| {
        let mut request = f.request(method, path, json!(null));
        let headers = request.headers_mut();
        headers.remove("origin");
        headers.remove("sec-fetch-site");
        if let Some(site) = site {
            headers.insert("sec-fetch-site", site.parse().unwrap());
        }
        headers.insert("cookie", cookie.parse().unwrap());
        f.send(request)
    };
    assert_eq!(bare("GET", "/__moss/source/doc.pdf", None).await.status(), StatusCode::OK);
    assert_eq!(bare("GET", "/__moss/source/doc.pdf", Some("cross-site")).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(bare("POST", "/__moss/source/doc.pdf", None).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(bare("GET", "/__moss/config", None).await.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_minted_link_comes_with_its_qr_code() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let mut request = f.request("POST", "/__moss/sign-in/new", json!({}));
    request.headers_mut().insert("cookie", cookie.parse().unwrap());
    let response = f.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap();
    let url = value["url"].as_str().unwrap();
    assert!(url.starts_with("https://editor.example.test/__moss/sign-in#code="), "{url}");
    assert_eq!(value["qr_svg"].as_str().unwrap(), crate::build::media::qr::generate_qr_svg(url).unwrap());
}

/// The preview is same-site with the UI, so a page on the preview origin
/// sends the session cookie with a request to the UI. The UI never admits
/// that origin, at loopback or behind the public URL.
#[tokio::test]
async fn the_ui_refuses_a_write_from_the_preview_origin() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    for (host, origin) in [
        ("editor.example.test", "https://editor.example.test:444"),
        ("localhost:62001", "http://localhost:62000"),
    ] {
        for site in ["same-site", "same-origin"] {
            let mut request = f.request("POST", "/__moss/mutate/save_editor_content", json!({}));
            let headers = request.headers_mut();
            headers.insert("host", host.parse().unwrap());
            headers.insert("origin", origin.parse().unwrap());
            headers.insert("sec-fetch-site", site.parse().unwrap());
            headers.insert("cookie", cookie.parse().unwrap());
            forbidden(f.send(request).await).await;
        }
    }
}

/// Under HTTPS the session cookie is `Secure`, both where it is set and
/// where each admitted request renews it; plain loopback HTTP cannot carry one.
#[tokio::test]
async fn the_session_cookie_is_secure_under_https() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let renewed = |host: &'static str, origin: &'static str| {
        let mut request = f.request("POST", PROBE, json!({}));
        let headers = request.headers_mut();
        headers.insert("host", host.parse().unwrap());
        headers.insert("origin", origin.parse().unwrap());
        headers.insert("cookie", cookie.parse().unwrap());
        let f = &f;
        async move { f.send(request).await.headers()["set-cookie"].to_str().unwrap().to_owned() }
    };
    assert!(renewed("editor.example.test", "https://editor.example.test").await.contains("; Secure"));
    assert!(!renewed("localhost:62001", ORIGIN).await.contains("Secure"));
}
