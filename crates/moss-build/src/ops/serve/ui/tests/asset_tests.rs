use super::*;

const TABLE: &[UiAsset] = &[
    UiAsset {
        path: "index.html",
        bytes: b"entry",
        content_type: "text/html; charset=utf-8",
        gzip: true,
        immutable: false,
    },
    UiAsset {
        path: "assets/app-0123456789ab.js",
        bytes: b"hashed",
        content_type: "text/javascript; charset=utf-8",
        gzip: true,
        immutable: true,
    },
    UiAsset {
        path: "plain.txt",
        bytes: b"plain",
        content_type: "text/plain; charset=utf-8",
        gzip: false,
        immutable: false,
    },
];

fn app(f: &Fixture) -> Router {
    super::super::router(
        UiOriginConfig {
            port: UiPort::Fixed(62001),
            auth: f.auth.clone(),
            site_root: f.site.clone(),
            public_url: None,
            assets: TABLE,
        },
        62001,
        62000,
        None,
        Arc::new(std::sync::RwLock::new(f.site.clone())),
        super::super::super::HostKind::Cli,
        Arc::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn ui_boundary_serves_the_asset_table_with_its_headers() {
    let f = Fixture::new();
    for (path, body, mime, cache, gzip) in [
        ("/", "entry", "text/html; charset=utf-8", "no-store", true),
        (
            "/assets/app-0123456789ab.js",
            "hashed",
            "text/javascript; charset=utf-8",
            "public, max-age=31536000, immutable",
            true,
        ),
        ("/plain.txt", "plain", "text/plain; charset=utf-8", "no-store", false),
    ] {
        let response = app(&f)
            .oneshot(f.request("GET", path, json!(null)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["cache-control"], cache, "{path}");
        assert_eq!(response.headers()["content-type"], mime, "{path}");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            response.headers().get("content-encoding").is_some_and(|v| v == "gzip"),
            gzip,
            "{path}"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], body.as_bytes());
    }
}

#[tokio::test]
async fn ui_assets_are_read_only_and_unknown_paths_are_not_found() {
    let f = Fixture::new();
    let missing = app(&f)
        .oneshot(f.request("GET", "/assets/other.js", json!(null)))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let post = app(&f)
        .oneshot(f.request("POST", "/", json!(null)))
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
}
