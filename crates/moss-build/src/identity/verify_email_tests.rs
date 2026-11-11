use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const TICK: Duration = Duration::from_millis(5);

/// A seta that refuses registration until the sign-in session has been polled
/// `verify_after` times, and records every request it saw.
pub(crate) struct Seta {
    pub(crate) log: Arc<Mutex<Vec<(String, String)>>>,
    pub(crate) polls: Arc<AtomicUsize>,
    pub(crate) url: String,
    /// What `/auth/whoami` says the key belongs to; `None` is a key linked to
    /// no account, as the service answers for one.
    pub(crate) account: Arc<Mutex<Option<&'static str>>>,
    /// Replies to the next sign-in requests, in order, as (status line,
    /// extra header lines, body); an empty queue answers 200.
    sign_in_replies: Arc<Mutex<std::collections::VecDeque<Reply>>>,
    /// The body of an unverified key's 403 on `/sync`.
    sync_refusal: Arc<Mutex<&'static str>>,
}

type Reply = (&'static str, &'static str, &'static str);

impl Seta {
    pub(crate) async fn start(verify_after: usize, final_status: &'static str) -> Seta {
        Seta::start_with(verify_after, final_status, None, 200).await
    }

    /// `status_email` is what the status response says opened the link;
    /// `status_http` is the HTTP code of every status poll.
    async fn start_with(
        verify_after: usize,
        final_status: &'static str,
        status_email: Option<&'static str>,
        status_http: u16,
    ) -> Seta {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let log: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let polls = Arc::new(AtomicUsize::new(0));
        let account: Arc<Mutex<Option<&'static str>>> = Arc::default();
        let sign_in_replies: Arc<Mutex<std::collections::VecDeque<Reply>>> = Arc::default();
        let sync_refusal = Arc::new(Mutex::new(r#"{"error":"Not authorized for this site"}"#));
        let (l, p, a, r, sr) = (log.clone(), polls.clone(), account.clone(), sign_in_replies.clone(), sync_refusal.clone());
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let raw = String::from_utf8_lossy(&crate::test_drain_http_request(&mut stream).await).to_string();
                let mut first = raw.lines().next().unwrap_or("").split_whitespace();
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                l.lock().unwrap().push((format!("{method} {path}"), body));
                let mut headers = "";
                let (code, json) = if path == "/auth/request" {
                    let (code, extra, json) = r.lock().unwrap().pop_front().unwrap_or(("200 OK", "", "{}"));
                    headers = extra;
                    (code, json.to_string())
                } else if path.starts_with("/auth/status/") {
                    let n = p.fetch_add(1, Ordering::SeqCst) + 1;
                    let status = if n >= verify_after { final_status } else { "pending" };
                    let email = status_email.map_or("null".to_string(), |e| format!("\"{e}\""));
                    match status_http {
                        200 => ("200 OK", format!(r#"{{"status":"{status}","email":{email}}}"#)),
                        403 => ("403 Forbidden", r#"{"error":"blocked by policy"}"#.to_string()),
                        _ => ("500 Internal Server Error", r#"{"error":"upstream down"}"#.to_string()),
                    }
                } else if path.ends_with("/sync") {
                    // A key no sign-in has verified is not the site's owner.
                    if p.load(Ordering::SeqCst) >= verify_after && final_status == "verified" {
                        ("200 OK", r#"{"need":[],"remove":[]}"#.to_string())
                    } else {
                        ("403 Forbidden", sr.lock().unwrap().to_string())
                    }
                } else if path == "/auth/whoami" {
                    let email = a.lock().unwrap().map_or("null".to_string(), |e| format!("\"{e}\""));
                    ("200 OK", format!(r#"{{"pubkey":"pk","email":{email}}}"#))
                } else if path == "/api/sites" {
                    if p.load(Ordering::SeqCst) >= verify_after && final_status == "verified" {
                        ("200 OK", r#"{"id":"my-site"}"#.to_string())
                    } else {
                        ("403 Forbidden", r#"{"error":"pubkey not verified for this email"}"#.to_string())
                    }
                } else {
                    ("404 Not Found", "{}".to_string())
                };
                let resp = format!(
                    "HTTP/1.1 {code}\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\n\r\n{json}",
                    json.len()
                );
                crate::test_respond_and_close(&mut stream, resp.as_bytes()).await;
            }
        });
        Seta { log, polls, url, account, sign_in_replies, sync_refusal }
    }

    fn reply_to_sign_in(&self, status: &'static str, headers: &'static str, body: &'static str) {
        self.sign_in_replies.lock().unwrap().push_back((status, headers, body));
    }

    /// The bodies of the sign-in requests, in order.
    pub(crate) fn sign_in_bodies(&self) -> Vec<serde_json::Value> {
        let log = self.log.lock().unwrap();
        log.iter().filter(|(l, _)| l == "POST /auth/request").map(|(_, b)| serde_json::from_str(b).unwrap()).collect()
    }

    /// Requests whose method and path are exactly `line`.
    pub(crate) fn count_exact(&self, line: &str) -> usize {
        self.log.lock().unwrap().iter().filter(|(l, _)| l == line).count()
    }

    pub(crate) fn count(&self, prefix: &str) -> usize {
        self.log.lock().unwrap().iter().filter(|(line, _)| line.starts_with(prefix)).count()
    }
}

/// A folder with a fresh identity, and `MOSS_SETA_URL` pointing at the mock.
pub(crate) struct Folder {
    pub(crate) dir: tempfile::TempDir,
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
    pub(crate) fn new(seta: &Seta) -> Folder {
        let env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        crate::identity::Identity::generate().unwrap().save(dir.path()).unwrap();
        let prev_url = std::env::var("MOSS_SETA_URL").ok();
        std::env::set_var("MOSS_SETA_URL", &seta.url);
        Folder { dir, _env: env, prev_url }
    }

    fn pubkey(&self) -> String {
        crate::identity::Identity::read_pubkey(self.dir.path()).unwrap()
    }

    async fn register(&self) -> Result<String, PublishError> {
        let identity = crate::identity::Identity::load_for_signing(self.dir.path()).unwrap();
        crate::deploy::register_moss_host_site(self.dir.path(), "my-site", &identity).await
    }
}

#[test]
fn email_addresses_are_shape_checked() {
    for good in ["a@b.co", "first.last@mail.example.org"] {
        assert!(validate_email(good).is_ok(), "{good}");
    }
    for bad in ["", "plain", "@b.co", "a@", "a@b", "a@@b.co", "a b@c.co", "a@.co", "a@b."] {
        assert!(validate_email(bad).is_err(), "{bad}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_without_email_names_the_flag_and_sends_nothing() {
    let seta = Seta::start(1, "verified").await;
    let folder = Folder::new(&seta);

    let err = retrying_after_verification(folder.dir.path(), None, None, TICK, WAIT_LIMIT, || folder.register())
        .await
        .expect_err("an unverified key is refused");

    assert!(matches!(err, PublishError::NotSignedIn { .. }), "{err:?}");
    let words = err.cli_text(folder.dir.path());
    assert!(words.ends_with("--email <address>"), "the refusal must name the flag: {words}");
    assert_eq!(seta.count("POST /auth/request"), 0, "no link may be sent without the flag");
}

/// `--email` on a folder whose key is gone signs it in with a new key: the link
/// is sent for the new key, which is what the folder then signs with.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_email_a_folder_that_lost_its_key_signs_in_a_new_one() {
    let seta = Seta::start(1, "verified").await;
    let folder = Folder::new(&seta);
    let lost = folder.pubkey();
    std::fs::remove_file(crate::identity::keypair::key_file_path(folder.dir.path())).unwrap();

    verify_folder_email(folder.dir.path(), "me@example.com", None, TICK, WAIT_LIMIT).await.expect("signed in");

    let fresh = folder.pubkey();
    assert_ne!(fresh, lost, "the lost key cannot be signed in");
    let body = seta.log.lock().unwrap().iter().find(|(l, _)| l == "POST /auth/request").unwrap().1.clone();
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["pubkey"], fresh);
    assert_eq!(crate::identity::Identity::key_on_disk(folder.dir.path()), crate::identity::keypair::KeyOnDisk::Current);
    assert_eq!(crate::identity::Identity::read_email(folder.dir.path()).as_deref(), Some("me@example.com"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_email_it_sends_one_link_polls_through_pending_records_the_email_and_retries() {
    let seta = Seta::start(3, "verified").await;
    let folder = Folder::new(&seta);

    let site = retrying_after_verification(folder.dir.path(), Some("me@example.com"), None, TICK, WAIT_LIMIT, || {
        folder.register()
    })
    .await
    .expect("verified, then registered");

    assert_eq!(site, "my-site");
    assert_eq!(seta.count("POST /auth/request"), 1);
    assert_eq!(seta.polls.load(Ordering::SeqCst), 3, "pending twice, then verified");
    assert_eq!(seta.count("POST /api/sites"), 2, "refused once, retried once");
    let body = seta.log.lock().unwrap().iter().find(|(l, _)| l == "POST /auth/request").unwrap().1.clone();
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["email"], "me@example.com");
    assert_eq!(sent["pubkey"], folder.pubkey());
    let session = sent["session_id"].as_str().unwrap();
    assert!(session.len() == 64 && session.chars().all(|c| c.is_ascii_hexdigit()), "{session}");
    let name = folder.dir.path().file_name().unwrap().to_str().unwrap();
    assert_eq!(sent["folder_name"], name, "the link names the folder being signed in");
    assert!(sent.get("site_id").is_none(), "a folder not yet deployed names no site: {sent}");
    assert_eq!(
        crate::identity::Identity::read_email(folder.dir.path()).as_deref(),
        Some("me@example.com"),
        "the verified address is recorded in the folder's identity"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_link_fails_with_what_to_do_and_does_not_retry() {
    let seta = Seta::start(2, "expired").await;
    let folder = Folder::new(&seta);

    let err = retrying_after_verification(folder.dir.path(), Some("me@example.com"), None, TICK, WAIT_LIMIT, || {
        folder.register()
    })
    .await
    .expect_err("an expired link is not a verification");

    assert!(err.to_string().contains("--email"), "say what to do: {err}");
    assert_eq!(seta.count("POST /api/sites"), 1, "no retry after a failed verification");
    assert_eq!(crate::identity::Identity::read_email(folder.dir.path()), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_nobody_opens_times_out() {
    let seta = Seta::start(usize::MAX, "verified").await;
    let folder = Folder::new(&seta);

    let err = retrying_after_verification(
        folder.dir.path(),
        Some("me@example.com"),
        None,
        TICK,
        Duration::from_millis(60),
        || folder.register(),
    )
    .await
    .expect_err("never opened");

    assert!(err.to_string().contains("No one opened the sign-in link"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_opened_by_another_account_fails_and_writes_nothing() {
    let seta = Seta::start_with(1, "verified", Some("other@example.test"), 200).await;
    let folder = Folder::new(&seta);

    let err = verify_folder_email(folder.dir.path(), "ada@example.test", None, TICK, Duration::from_secs(2))
        .await
        .expect_err("the link was opened for someone else");

    assert!(err.contains("other@example.test") && err.contains("ada@example.test"), "{err}");
    assert!(err.contains("Nothing was changed") && err.contains("again"), "{err}");
    assert_eq!(crate::identity::Identity::read_email(folder.dir.path()), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_status_email_matches_the_typed_one_ignoring_case_and_padding() {
    let seta = Seta::start_with(1, "verified", Some(" ADA@Example.test "), 200).await;
    let folder = Folder::new(&seta);

    verify_folder_email(folder.dir.path(), "ada@example.test", None, TICK, Duration::from_secs(2)).await.expect("same account");

    assert_eq!(crate::identity::Identity::read_email(folder.dir.path()).as_deref(), Some("ada@example.test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_status_check_the_service_keeps_refusing_stops_early_and_says_what_it_said() {
    let seta = Seta::start_with(usize::MAX, "verified", None, 403).await;
    let folder = Folder::new(&seta);

    let err = verify_folder_email(folder.dir.path(), "ada@example.test", None, TICK, Duration::from_secs(2))
        .await
        .expect_err("a permanent refusal");

    assert!(err.contains("refused") && err.contains("blocked by policy"), "{err}");
    assert_eq!(seta.polls.load(Ordering::SeqCst), 5, "five refusals in a row, then stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_that_never_answers_a_poll_is_reported_unreachable_not_unopened() {
    let seta = Seta::start_with(usize::MAX, "verified", None, 500).await;
    let folder = Folder::new(&seta);

    let err = verify_folder_email(folder.dir.path(), "ada@example.test", None, TICK, Duration::from_millis(100))
        .await
        .expect_err("no poll ever succeeded");

    assert!(err.contains("could not be reached") && err.contains("upstream down"), "{err}");
    assert!(!err.contains("No one opened"), "{err}");
}

#[test]
fn the_replaced_address_notice_names_both_addresses() {
    let notice = replaced_email_notice("old@example.test", "ada@example.test");
    assert!(notice.contains("old@example.test") && notice.contains("ada@example.test"), "{notice}");
    assert!(notice.contains("replac"), "{notice}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verifying_over_a_stored_address_replaces_it() {
    let seta = Seta::start(1, "verified").await;
    let folder = Folder::new(&seta);
    let mut identity = crate::identity::Identity::load_for_signing(folder.dir.path()).unwrap();
    identity.link_email("old@example.test".to_string());
    identity.save(folder.dir.path()).unwrap();

    verify_folder_email(folder.dir.path(), "ada@example.test", None, TICK, Duration::from_secs(2)).await.unwrap();

    assert_eq!(crate::identity::Identity::read_email(folder.dir.path()).as_deref(), Some("ada@example.test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unlinked_key_is_sent_a_link_only_for_a_not_the_owner_refusal() {
    let seta = Seta::start(1, "verified").await;
    let folder = Folder::new(&seta);
    let refused = |message: &'static str| move || async move { Err::<(), PublishError>(message.to_string().into()) };

    let _ = retrying_after_verification(folder.dir.path(), Some("ada@example.test"), None, TICK, WAIT_LIMIT, refused("HTTP request failed: connection refused")).await;
    assert_eq!(seta.count("POST /auth/request"), 0, "a refusal for another reason sends nothing");

    let _ = retrying_after_verification(folder.dir.path(), Some("ada@example.test"), None, TICK, WAIT_LIMIT, refused("API error: 403 - Not authorized for this site")).await;
    assert_eq!(seta.count("POST /auth/request"), 1);
}

#[test]
fn a_deployed_folder_names_its_site_and_never_its_path() {
    let parent = tempfile::tempdir().unwrap();
    let dir = parent.path().join("Blog");
    std::fs::create_dir(&dir).unwrap();
    let config = crate::config::deployment::DomainDeploymentConfig { site_id: Some("her-blog".into()), ..Default::default() };
    crate::vault::deployment_state::save_domain_config(dir.to_str().unwrap(), &config).unwrap();

    let named = sign_in_folder(&dir, None);
    assert_eq!(named.site_id.as_deref(), Some("her-blog"));
    assert_eq!(named.name.as_deref(), Some("Blog"));
    assert!(sign_in_folder(parent.path(), None).site_id.is_none(), "an undeployed folder names no site");
}

const KEY_HAS_OTHER_SITE: &str = r#"{"error":"This folder's key is signed in to old-blog.mosspub.com. Update moss to sign this folder in with a key of its own.","code":"key_has_other_site","site":"old-blog.mosspub.com"}"#;
const JUST_SENT: &str = r#"{"error":"A verification email was just sent. You can request another in a moment."}"#;

/// A folder copied from another one holds that folder's key, which the server
/// has signed in to the other folder's site. The sign-in gives this folder a
/// key of its own, keeps the copy among its retired keys, waits out the wait
/// the server names for the address, and asks again for the new key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_copied_key_is_replaced_by_one_of_the_folders_own_and_the_link_asked_for_again() {
    let seta = Seta::start(1, "verified").await;
    seta.reply_to_sign_in("409 Conflict", "", KEY_HAS_OTHER_SITE);
    seta.reply_to_sign_in("429 Too Many Requests", "Retry-After: 0\r\n", JUST_SENT);
    let folder = Folder::new(&seta);
    let copied = folder.pubkey();

    verify_folder_email(folder.dir.path(), "me@example.com", None, TICK, WAIT_LIMIT).await.expect("signed in");

    let own = folder.pubkey();
    assert_ne!(own, copied);
    let sent: Vec<String> = seta.sign_in_bodies().iter().map(|b| b["pubkey"].as_str().unwrap().to_string()).collect();
    assert_eq!(sent, [copied.clone(), own.clone(), own]);
    let retired = std::fs::read_to_string(folder.dir.path().join(".moss/identity/previous-keys.jsonl")).unwrap();
    assert!(retired.contains(&copied), "{retired}");
    assert_eq!(crate::identity::Identity::read_email(folder.dir.path()).as_deref(), Some("me@example.com"));
}

/// A wait longer than a sign-in sits out is the server's sentence, not a hang.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_wait_after_a_copied_key_is_reported() {
    let seta = Seta::start(1, "verified").await;
    seta.reply_to_sign_in("409 Conflict", "", KEY_HAS_OTHER_SITE);
    seta.reply_to_sign_in("429 Too Many Requests", "Retry-After: 3600\r\n", JUST_SENT);
    let folder = Folder::new(&seta);

    let err = verify_folder_email(folder.dir.path(), "me@example.com", None, TICK, WAIT_LIMIT).await.expect_err("too soon");
    assert!(err.contains("just sent"), "{err}");
    assert_eq!(seta.count("POST /auth/request"), 2);
}

/// A publish refused because the folder isn't signed in to its site, by the
/// owner's key signed in elsewhere or nowhere, is the sentence `--email`
/// lifts: the link is sent without first asking whose key it is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_not_signed_in_to_its_site_is_signed_in_by_email() {
    let seta = Seta::start(1, "verified").await;
    *seta.sync_refusal.lock().unwrap() = r#"{"error":"Not authorized for this site","code":"folder_not_signed_in","site":"my-site.mosspub.com","detail":"This folder isn't signed in to my-site.mosspub.com."}"#;
    let folder = Folder::new(&seta);
    let identity = crate::identity::Identity::load_for_signing(folder.dir.path()).unwrap();
    let client = crate::seta::client::MossSetaClient::for_environment(&identity, &crate::build::site_config::resolve_environment(&folder.dir.path().to_string_lossy()));
    let sync = || async {
        crate::deploy::profile::sync(client.sync_manifest("my-site", &Default::default(), &Default::default(), "gen")).await.map(|_| ())
    };

    let err = sync().await.expect_err("not signed in");
    assert_eq!(err, PublishError::NotSignedIn { host: "my-site.mosspub.com".into() });

    retrying_after_verification(folder.dir.path(), Some("me@example.com"), None, TICK, WAIT_LIMIT, sync).await.expect("signed in, then synced");
    assert_eq!(seta.count("POST /auth/request"), 1);
    assert_eq!(seta.count("GET /auth/whoami"), 0, "the refusal itself says --email lifts it");
}
