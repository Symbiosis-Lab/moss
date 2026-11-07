use super::*;
use crate::identity::Identity;
use std::sync::{Arc, Mutex};

/// A hosting service that answers transfer requests with `reply`, recording
/// every request line and body.
struct FakeSeta {
    url: String,
    log: Arc<Mutex<Vec<(String, String)>>>,
    reply: Arc<Mutex<(&'static str, &'static str)>>,
}

const SENT: &str = r#"{"status":"sent","expires_at":1900000000,"plan":{"site_id":"my-site","from":"old@example.com","to":"new@example.com","domains":["example.org"],"subscription":{"plan":"annual","origin":"comp"},"keep_publishing":false}}"#;

impl FakeSeta {
    async fn start() -> FakeSeta {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let reply = Arc::new(Mutex::new(("201 Created", SENT)));
        let (l, r) = (log.clone(), reply.clone());
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let raw = String::from_utf8_lossy(&crate::test_drain_http_request(&mut stream).await).to_string();
                let mut first = raw.lines().next().unwrap_or("").split_whitespace();
                let line = format!("{} {}", first.next().unwrap_or(""), first.next().unwrap_or(""));
                l.lock().unwrap().push((line.clone(), raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string()));
                let (code, json) = if line == "POST /api/sites/my-site/transfer" {
                    let (code, body) = *r.lock().unwrap();
                    (code, body.to_string())
                } else if line == "DELETE /api/sites/my-site/transfer" {
                    ("200 OK", r#"{"status":"cancelled"}"#.to_string())
                } else {
                    ("404 Not Found", "{}".to_string())
                };
                let resp = format!("HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{json}", json.len());
                crate::test_respond_and_close(&mut stream, resp.as_bytes()).await;
            }
        });
        FakeSeta { url, log, reply }
    }

    fn count(&self, line: &str) -> usize {
        self.log.lock().unwrap().iter().filter(|(l, _)| l == line).count()
    }

    fn body_of(&self, line: &str) -> serde_json::Value {
        let log = self.log.lock().unwrap();
        serde_json::from_str(&log.iter().find(|(l, _)| l == line).expect("request was sent").1).unwrap()
    }
}

/// A deployed folder with its own key, `MOSS_SETA_URL` pointing at the fake.
struct Folder {
    dir: tempfile::TempDir,
    _env: std::sync::MutexGuard<'static, ()>,
    prev_url: Option<String>,
}

impl Drop for Folder {
    fn drop(&mut self) {
        match self.prev_url.take() {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }
    }
}

impl Folder {
    fn new(seta: &FakeSeta) -> Folder {
        let env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mut identity = Identity::generate().unwrap();
        identity.link_email("old@example.com".into());
        identity.save(dir.path()).unwrap();
        std::fs::write(dir.path().join(".moss").join("state.toml"), "[deployment]\nsite_id = \"my-site\"\n").unwrap();
        let prev_url = std::env::var("MOSS_SETA_URL").ok();
        std::env::set_var("MOSS_SETA_URL", &seta.url);
        Folder { dir, _env: env, prev_url }
    }

    fn args(&self, rest: &[&str]) -> TransferArgs {
        let mut v = vec![self.dir.path().to_string_lossy().to_string()];
        v.extend(rest.iter().map(|s| s.to_string()));
        parse(&v).unwrap()
    }
}

#[test]
fn parse_wants_one_mode_and_a_real_address() {
    let p = |a: &[&str]| parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(p(&["f", "--to=a@b.co"]).unwrap().mode, Mode::To { email: "a@b.co".into(), keep_publishing: false, dry_run: false });
    assert_eq!(p(&["f", "--cancel"]).unwrap().mode, Mode::Cancel);
    assert!(p(&["f", "--to", "a@b.co", "--keep-publishing", "--dry-run", "--json"]).is_ok());
    for bad in [&["f"][..], &["f", "--to", "a@b.co", "--cancel"], &["f", "--cancel", "--dry-run"], &["--to", "a@b.co"], &["f", "--to", "nope"], &["f", "--to"], &["f", "--status"]] {
        assert!(p(bad).is_err(), "{bad:?}");
    }
    assert!(p(&["f", "--to", "nope"]).unwrap_err().contains("'nope' is not an email address"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transfer_sends_one_signed_request_and_the_folder_key_stays() {
    let seta = FakeSeta::start().await;
    let folder = Folder::new(&seta);
    let before = Identity::read_pubkey(folder.dir.path());

    assert_eq!(transfer(folder.args(&["--to", "new@example.com"])).await, 0);

    let body = seta.body_of("POST /api/sites/my-site/transfer");
    assert_eq!((body["to"].as_str(), body["dry_run"].as_bool(), body["keep_publishing"].as_bool()), (Some("new@example.com"), Some(false), Some(false)));
    assert_eq!(body["client"]["name"], "moss");
    assert_eq!(Identity::read_pubkey(folder.dir.path()), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_publishing_and_dry_run_are_sent_as_asked() {
    let seta = FakeSeta::start().await;
    let folder = Folder::new(&seta);

    assert_eq!(transfer(folder.args(&["--to", "new@example.com", "--keep-publishing", "--dry-run"])).await, 0);

    let body = seta.body_of("POST /api/sites/my-site/transfer");
    assert_eq!((body["dry_run"].as_bool(), body["keep_publishing"].as_bool()), (Some(true), Some(true)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_key_refusal_names_the_other_sites() {
    let seta = FakeSeta::start().await;
    let folder = Folder::new(&seta);
    *seta.reply.lock().unwrap() = ("409 Conflict", r#"{"error":"key_shared","detail":"this folder's key also publishes second-site; transfer those first or give that folder its own key"}"#);
    let client = crate::cli::client_for_folder(folder.dir.path()).unwrap();

    let e = client.request_transfer("my-site", "new@example.com", false, true).await.unwrap_err();
    let out = refused(true, e);

    assert_eq!(out.code, 2);
    let v: serde_json::Value = serde_json::from_str(&out.out).unwrap();
    assert_eq!(v["code"], "key_shared");
    assert!(v["error"].as_str().unwrap().contains("second-site"), "{v}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_withdraws_the_transfer() {
    let seta = FakeSeta::start().await;
    let folder = Folder::new(&seta);
    assert_eq!(transfer(folder.args(&["--cancel"])).await, 0);
    assert_eq!(seta.count("DELETE /api/sites/my-site/transfer"), 1);
}
