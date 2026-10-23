//! What a failing status poll writes to the log: the session id is a bearer for
//! the status route and the log is uploaded by "Send logs", so it must not
//! appear. Its own test binary because `log::set_logger` is one-shot.

use std::sync::Mutex;

use moss_build::identity::Identity;
use moss_build::seta::client::MossSetaClient;

struct Capture(Mutex<Vec<(log::Level, String)>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push((record.level(), record.args().to_string()));
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

async fn serve(statuses: &'static [&'static str]) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        for status in statuses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let body = r#"{"error":"nope"}"#;
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(resp.as_bytes()).await.ok();
            stream.shutdown().await.ok();
        }
    });
    url
}

#[tokio::test]
async fn a_failed_poll_logs_no_session_id() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    let url = serve(&["429 Too Many Requests", "410 Gone"]).await;
    let identity = Identity::generate().unwrap();
    let client = MossSetaClient::with_identity_and_url(&identity, &url);
    let session = "0123456789abcdef".repeat(4);

    assert!(client.check_auth_status(&session).await.is_err());
    assert!(client.check_auth_status(&session).await.is_err());

    let lines = CAPTURE.0.lock().unwrap().clone();
    let seta: Vec<_> = lines.iter().filter(|(_, l)| l.contains("/auth/status/")).collect();
    assert_eq!(seta.len(), 2, "{lines:?}");
    assert!(seta.iter().all(|(_, l)| !l.contains(&session)), "session id leaked: {seta:?}");
    assert!(seta.iter().any(|(lvl, l)| *lvl == log::Level::Warn && l.contains("(429)")), "{seta:?}");
}
