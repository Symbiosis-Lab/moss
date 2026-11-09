use super::super::{ownership, start_server, InvokeCtx, ServeConfig};
use super::*;
use sign_in::SignInLink;
use std::sync::Arc;
use std::sync::RwLock;

fn config(root: &std::path::Path) -> ServeConfig {
    let site = root.join("site");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    let auth = SignInLink::new(root.join("sessions.json"), &site).unwrap();
    ServeConfig {
        ui: Some(UiOriginConfig {
            port: UiPort::ScanFrom(63120),
            auth: Arc::new(auth),
            public_url: None,
            site_root: site.clone(),
            assets: &[UiAsset {
                path: "index.html",
                bytes: b"UI entry",
                content_type: "text/html; charset=utf-8",
                gzip: false,
                immutable: false,
            }],
        }),
        invoke: Some(InvokeCtx::standalone()),
        ..ServeConfig::new(Arc::new(RwLock::new(site)), 63100)
    }
}

#[tokio::test]
async fn server_validates_session_location_before_listening() {
    let root = tempfile::tempdir().unwrap();
    let mut config = config(root.path());
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    config.ui.as_mut().unwrap().auth =
        Arc::new(SignInLink::new(root.path().join("site/.moss/sessions.json"), &outside).unwrap());
    assert_eq!(
        start_server(config).await.unwrap_err(),
        "session file must be outside the served folder"
    );
}

#[test]
fn ui_listener_is_separate_and_recorded_and_shuts_down() {
    let _guard = crate::infra::home::MOSS_HOME_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let root = tempfile::tempdir().unwrap();
            let config = config(root.path());
            let site = config.site_dir_cell.read().unwrap().clone();
            let link = config.ui.as_ref().unwrap().auth.mint_sign_in().unwrap();
            let (preview, ui, shutdown) = start_server(config).await.unwrap();
            let ui = ui.expect("a UI port must be returned");
            assert_ne!(preview, ui);
            let owner = ownership::find_live_owner(&site).await.unwrap();
            assert_eq!(owner.ui_url, Some(format!("http://127.0.0.1:{ui}")));
            let result = tokio::task::spawn_blocking(move || {
                ureq::get(&format!("http://127.0.0.1:{ui}/"))
                    .set("Sec-Fetch-Site", "none")
                    .set("Sec-Fetch-Mode", "navigate")
                    .set("Sec-Fetch-Dest", "document")
                    .call()
                    .unwrap()
                    .into_string()
                    .unwrap()
            })
            .await
            .unwrap();
            assert_eq!(result, "UI entry");
            tokio::task::spawn_blocking(move || {
                assert_ne!(get_as(preview, &format!("127.0.0.1:{preview}"), "/", None).0, 401);
                let origin = format!("http://127.0.0.1:{ui}");
                let response = ureq::post(&format!("{origin}/__moss/sign-in"))
                    .set("Origin", &origin)
                    .set("Sec-Fetch-Site", "same-origin")
                    .set("Content-Type", "application/json")
                    .send_string(&serde_json::json!({"code": link.code}).to_string())
                    .unwrap();
                assert_eq!(response.status(), 204);
                let cookie = response
                    .header("set-cookie")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap();
                for (port, expected) in [(ui, 404), (preview, 401)] {
                    let result = ureq::post(&format!(
                        "http://127.0.0.1:{port}/__moss/read/not_a_command"
                    ))
                    .set("Origin", &format!("http://127.0.0.1:{port}"))
                    .set("Sec-Fetch-Site", "same-origin")
                    .set("Content-Type", "application/json")
                    .set("Cookie", cookie)
                    .send_string("{}");
                    assert!(
                        matches!(result, Err(ureq::Error::Status(status, _)) if status == expected)
                    );
                }
            })
            .await
            .unwrap();

            shutdown.send(()).unwrap();
            for _ in 0..50 {
                if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, ui))
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            panic!("UI listener survived server shutdown");
        });
}

/// A request with the `Host` a proxy would forward, which a URL-based client
/// derives from the address instead.
fn get_as(port: u16, host: &str, path: &str, cookie: Option<&str>) -> (u16, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {c}\r\n"));
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nSec-Fetch-Site: same-origin\r\n{cookie}Connection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    (response.get(9..12).unwrap().parse().unwrap(), response)
}

/// A proxy may forward with any Host, so behind a public URL the preview
/// cannot tell a remote reader from a local one by the request alone.
#[test]
fn behind_a_public_url_the_preview_asks_for_a_session_on_every_host() {
    let _guard = crate::infra::home::MOSS_HOME_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let root = tempfile::tempdir().unwrap();
            let mut config = config(root.path());
            config.start_port = 63200;
            let site = config.site_dir_cell.read().unwrap().clone();
            std::fs::write(site.join("index.html"), "draft").unwrap();
            let ui_config = config.ui.as_mut().unwrap();
            ui_config.port = UiPort::ScanFrom(63220);
            ui_config.public_url = Some(PublicUrl::new(PublicOrigin::parse("https://edit.example.test:9443").unwrap(), None).unwrap());
            let link = ui_config.auth.mint_sign_in().unwrap();
            let (preview, ui, shutdown) = start_server(config).await.unwrap();
            let ui = ui.unwrap();
            let owned = site.clone();
            tokio::task::spawn_blocking(move || {
                let (status, body) = get_as(ui, "edit.example.test:9443", "/__moss/config", None);
                assert_eq!(status, 200, "{body}");
                assert!(body.contains(r#""preview_url":"https://edit.example.test:9444""#), "{body}");
                let (_, body) = get_as(ui, &format!("127.0.0.1:{ui}"), "/__moss/config", None);
                assert!(body.contains(&format!(r#""preview_url":"http://127.0.0.1:{preview}""#)), "{body}");

                // The UI's own session check has no Host condition either.
                let origin = format!("http://127.0.0.1:{ui}");
                let carrier = |cookie: Option<&str>| {
                    let request = ureq::post(&format!("{origin}/__moss/read/not_a_command"))
                        .set("Origin", &origin)
                        .set("Sec-Fetch-Site", "same-origin")
                        .set("Content-Type", "application/json");
                    let request = match cookie {
                        Some(cookie) => request.set("Cookie", cookie),
                        None => request,
                    };
                    match request.send_string("{}") {
                        Err(ureq::Error::Status(status, _)) => status,
                        other => panic!("{other:?}"),
                    }
                };
                assert_eq!(carrier(None), 401);

                let local = format!("127.0.0.1:{preview}");
                for host in ["edit.example.test:9444", local.as_str()] {
                    assert_eq!(get_as(preview, host, "/", None).0, 401, "{host}");
                    let (status, body) = get_as(preview, host, super::super::port::MOSS_HEALTH_PATH, None);
                    assert_eq!(status, 200, "{host}: {body}");
                    assert!(body.ends_with(r#"{"server":"moss-preview-server","schema":2}"#), "{body}");
                }
                assert_eq!(get_as(preview, "edit.example.test:9443", "/", None).0, 421);

                let response = ureq::post(&format!("{origin}/__moss/sign-in"))
                    .set("Origin", &origin)
                    .set("Sec-Fetch-Site", "same-origin")
                    .set("Content-Type", "application/json")
                    .send_string(&serde_json::json!({"code": link.code}).to_string())
                    .unwrap();
                let cookie = response.header("set-cookie").unwrap().split(';').next().unwrap().to_owned();
                assert_eq!(carrier(Some(&cookie)), 404);
                for host in ["edit.example.test:9444", local.as_str()] {
                    let (status, body) = get_as(preview, host, "/", Some(&cookie));
                    assert_eq!(status, 200, "{host}: {body}");
                }

                // A local moss process proves itself with the carrier token it reads from the folder.
                let with_token = |request: ureq::Request, token: &str| match request.set(super::super::carrier_token::TOKEN_HEADER, token).call() {
                    Ok(response) => (response.status(), response.into_string().unwrap()),
                    Err(ureq::Error::Status(status, _)) => (status, String::new()),
                    Err(e) => panic!("{e}"),
                };
                let mint = || ureq::post(&format!("http://{local}/__moss/sign-in/new"));
                let health = || ureq::get(&format!("http://{local}{}", super::super::port::MOSS_HEALTH_PATH));
                let token = std::fs::read_to_string(super::super::carrier_token::token_path(&site)).unwrap();
                assert_eq!(with_token(mint(), "not-the-token").0, 401);
                assert_eq!(with_token(health(), "not-the-token").0, 401);
                assert_eq!(with_token(mint(), token.trim()).0, 200);
                let (status, body) = with_token(health(), token.trim());
                assert_eq!(status, 200);
                assert!(body.contains(r#""folder_id":"#), "{body}");
            })
            .await
            .unwrap();
            assert!(ownership::find_live_owner(&owned).await.is_some(), "a second moss edit finds the owner with the token");
            shutdown.send(()).unwrap();
        });
}

/// A proxy forwards to the preview's port, so behind a public URL the
/// preview never moves to another one: a taken port fails, naming it.
#[tokio::test]
async fn behind_a_public_url_a_taken_preview_port_fails_by_name() {
    let root = tempfile::tempdir().unwrap();
    let held = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let taken = held.local_addr().unwrap().port();
    let mut config = config(root.path());
    config.start_port = taken;
    config.ui.as_mut().unwrap().public_url =
        Some(PublicUrl::new(PublicOrigin::parse("https://edit.example.test").unwrap(), None).unwrap());
    let error = start_server(config).await.unwrap_err();
    assert!(error.contains(&format!("port {taken}")), "{error}");
}
