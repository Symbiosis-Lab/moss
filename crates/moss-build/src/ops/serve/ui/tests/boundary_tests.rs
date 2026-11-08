use super::*;

#[tokio::test]
async fn preview_origin_cookie_is_forbidden() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    assert_eq!(f.probe(&cookie).await, StatusCode::NOT_FOUND);
    let mut request = f.request("POST", PROBE, json!({}));
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "http://localhost:62000".parse().unwrap());
    forbidden(f.send(request).await).await;
}

#[tokio::test]
async fn same_site_cookie_is_forbidden() {
    fetch_site_refused("same-site").await;
}

#[tokio::test]
async fn cross_site_cookie_is_forbidden() {
    fetch_site_refused("cross-site").await;
}

#[tokio::test]
async fn missing_fetch_metadata_is_forbidden() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let mut request = f.request("POST", PROBE, json!({}));
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request.headers_mut().remove("sec-fetch-site");
    forbidden(f.send(request).await).await;
}

async fn fetch_site_refused(value: &str) {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let mut request = f.request("POST", PROBE, json!({}));
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("sec-fetch-site", value.parse().unwrap());
    forbidden(f.send(request).await).await;
}

#[tokio::test]
async fn multipart_upload_from_preview_is_forbidden() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    let multipart = "--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"attack.md\"\r\nContent-Type: text/markdown\r\n\r\nattack\r\n--boundary--\r\n";
    let mut request = f.request("POST", "/__moss/upload", json!(null));
    *request.body_mut() = Body::from(multipart);
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request.headers_mut().insert(
        "content-type",
        "multipart/form-data; boundary=boundary".parse().unwrap(),
    );
    request
        .headers_mut()
        .insert("origin", "http://localhost:62000".parse().unwrap());
    forbidden(f.send(request).await).await;
    assert!(!f.site.join("attack.md").exists());
}

#[tokio::test]
async fn default_allows_configured_remote_origin() {
    let f = Fixture::new();
    let mut request = f.request("GET", "/", json!(null));
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.1:1234".parse::<std::net::SocketAddr>().unwrap(),
    ));
    request
        .headers_mut()
        .insert("host", "editor.example.test".parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "https://editor.example.test".parse().unwrap());
    assert_eq!(f.send(request).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn navigation_exception_is_only_entry_get() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    for (method, path, expected) in [
        ("GET", "/", StatusCode::OK),
        ("GET", "/bundle.js", StatusCode::FORBIDDEN),
        ("POST", "/", StatusCode::FORBIDDEN),
        ("GET", "/__moss/auth/challenge", StatusCode::FORBIDDEN),
    ] {
        let mut request = f.request(method, path, json!(null));
        request.headers_mut().remove("origin");
        for (name, value) in [
            ("cookie", cookie.as_str()),
            ("sec-fetch-site", "none"),
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-dest", "document"),
        ] {
            request.headers_mut().insert(name, value.parse().unwrap());
        }
        assert_eq!(f.send(request).await.status(), expected, "{method} {path}");
    }
}

#[tokio::test]
async fn carrier_requires_cookie_and_changes_require_post() {
    let f = Fixture::new();
    let (_, cookie) = f.enrolled().await;
    assert_eq!(
        f.send(f.request("POST", PROBE, json!({}))).await.status(),
        StatusCode::UNAUTHORIZED
    );
    for path in [
        "/__moss/mutate/create_files",
        "/__moss/upload",
        "/__moss/yield",
        "/__moss/sign-out",
    ] {
        let mut request = f.request("GET", path, json!({}));
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        assert_eq!(
            f.send(request).await.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{path}"
        );
    }
}

#[tokio::test]
async fn unlisted_host_is_refused() {
    let f = Fixture::new();
    let mut request = f.request("GET", "/", json!(null));
    request.headers_mut().remove("origin");
    request
        .headers_mut()
        .insert("host", "rebound.example.test:62001".parse().unwrap());
    assert_eq!(
        f.send(request).await.status(),
        StatusCode::MISDIRECTED_REQUEST
    );
}
