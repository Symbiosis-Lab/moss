use super::{auth::UiAuth, sign_in::SignInLink, *};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

mod boundary_tests;
mod sign_in_tests;
mod revision_tests;
mod asset_tests;

const ASSETS: &[UiAsset] = &[
    UiAsset { path: "index.html", bytes: b"entry", content_type: "text/html; charset=utf-8", gzip: false, immutable: false },
    UiAsset { path: "bundle.js", bytes: b"bundle", content_type: "text/javascript; charset=utf-8", gzip: false, immutable: false },
];
const ORIGIN: &str = "http://localhost:62001";
const PROBE: &str = "/__moss/invoke/not_a_command";

struct Fixture {
    _root: tempfile::TempDir,
    auth: Arc<SignInLink>,
    app: Router,
    session_path: std::path::PathBuf,
    site: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let site = root.path().join("site");
        // allow:raw_write a fixture site in a fresh temp dir
        std::fs::create_dir_all(site.join(".moss")).unwrap();
        let session_path = root.path().join("machine/sessions.json");
        let auth = Arc::new(SignInLink::new(session_path.clone(), &site).unwrap());
        let config = super::UiOriginConfig {
            port: super::UiPort::Fixed(62001),
            auth: auth.clone(),
            site_root: site.clone(),
            assets: ASSETS,
            public_url: Some(PublicOrigin::parse("https://editor.example.test").unwrap()),
        };
        let app = super::router(
            config,
            62001,
            "https://preview.example.test".into(),
            Some(super::super::InvokeCtx::standalone()),
            Arc::new(std::sync::RwLock::new(site.clone())),
            super::super::HostKind::Cli,
            Arc::default(),
        )
        .unwrap();
        Self {
            _root: root,
            auth,
            app,
            session_path,
            site,
        }
    }

    fn request(&self, method: &str, path: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost:62001")
            .header("origin", ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:1234".parse::<std::net::SocketAddr>().unwrap(),
            ))
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn send(&self, request: Request<Body>) -> Response {
        self.app.clone().oneshot(request).await.unwrap()
    }

    async fn exchange(&self, code: &str) -> Response {
        self.send(self.request("POST", "/__moss/sign-in", json!({"code":code})))
            .await
    }

    async fn enrolled(&self) -> (String, String) {
        let code = self.auth.mint_sign_in().unwrap().code;
        let response = self.exchange(&code).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        (self.auth.list_sessions().unwrap()[0].id.clone(), cookie)
    }

    async fn probe(&self, cookie: &str) -> StatusCode {
        let mut request = self.request("POST", PROBE, json!({}));
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        self.send(request).await.status()
    }
}

async fn forbidden(response: Response) {
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.headers().get("set-cookie").is_none());
}
