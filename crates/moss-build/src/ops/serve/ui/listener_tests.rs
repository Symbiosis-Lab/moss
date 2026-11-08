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
