//! What a publish through seta's deploy resource (`POST /api/sites/:id/deploys`,
//! then `.../deploys/:deploy/files` and `.../deploys/:deploy/commit`) must keep
//! of the route it replaces (`/sync`, one `PUT /files` per file, `/commit`).
//!
//! The stand-in below serves the deploy resource and nothing of the old route.
//! Each test names its counterpart on the old route. The dry run and
//! `NeedsSetup` hold on both routes; their tests live beside that code.

use super::{push_site_inner, PushContext};
use crate::build::manifest::{HashBucket, PendingManifest, SealedManifest};
use crate::types::content::SiteHashes;
use crate::build::served_path::ServedPath;
use crate::deploy::progress::{self, DeployStage, RecordingSink};
use crate::deploy::PushResult;
use crate::moss_paths::MossPaths;
use crate::seta::client::tests::Received;
use crate::identity::Identity;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const SITE: &str = "parity-test";

/// What the stand-in does with one request: answer it, or drop the connection.
pub(crate) enum Reply {
    Json(u16, serde_json::Value),
    Drop,
    /// The stand-in handles the request, then answers with this status: a
    /// response lost after the server staged its parts.
    Staged(u16),
}

/// Lets a test answer a request itself; `n` counts the requests received
/// before this one. `None` leaves it to the stand-in.
pub(crate) type Fault = dyn Fn(&Received, usize) -> Option<Reply> + Send + Sync;

/// What the stand-in serves besides the deploy resource's own answers.
#[derive(Clone)]
pub(crate) struct Seta {
    /// The largest file a send takes as a part.
    pub request_bytes: u64,
    /// The answer to `GET .../generation`; `None` answers 404.
    pub generation: Option<serde_json::Value>,
    /// Fields that override a commit's answer, such as `url` or `files_updated`.
    pub committed: serde_json::Value,
    /// The deploy a manifest names is already what the site serves.
    pub already_live: bool,
    /// The `cf-ray` every answer carries.
    pub ray: Option<&'static str>,
}

impl Default for Seta {
    fn default() -> Self {
        Self { request_bytes: 4 << 20, generation: None, committed: serde_json::json!({}), already_live: false, ray: None }
    }
}

/// The open deploy: its id, and path -> (entry, size) of what it still needs;
/// and the deploy the site serves.
#[derive(Default)]
struct Pending {
    deploy: String,
    needed: BTreeMap<String, (String, u64)>,
    files: usize,
    live: String,
}

/// seta with nothing of the old route: it answers 404 to `/sync` and `/commit`.
pub(crate) struct DeployStandIn {
    pub url: String,
    pub addr: std::net::SocketAddr,
    seen: Arc<Mutex<Vec<Received>>>,
}

impl DeployStandIn {
    /// seta with nothing live, taking sends of up to `request_bytes`.
    pub(crate) async fn start(request_bytes: u64, fault: Box<Fault>) -> Self {
        Self::serve(Seta { request_bytes, ..Seta::default() }, fault).await
    }

    pub(crate) async fn serve(seta: Seta, fault: Box<Fault>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            let state = Mutex::new(Pending::default());
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                let raw = crate::test_drain_http_request(&mut stream).await;
                if raw.is_empty() {
                    continue;
                }
                let req = Received::from_raw(&raw);
                let n = { let mut l = log.lock().unwrap(); l.push(req.clone()); l.len() - 1 };
                let reply = match fault(&req, n) {
                    Some(Reply::Staged(status)) => { answer(&req, &state, &seta); Reply::Json(status, serde_json::json!({})) }
                    other => other.unwrap_or_else(|| answer(&req, &state, &seta)),
                };
                match reply {
                    Reply::Drop | Reply::Staged(_) => drop(stream),
                    Reply::Json(status, body) => {
                        let body = body.to_string();
                        let ray = seta.ray.map(|r| format!("cf-ray: {r}\r\n")).unwrap_or_default();
                        let resp = format!("HTTP/1.1 {status} X\r\n{ray}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                        crate::test_respond_and_close(&mut stream, resp.as_bytes()).await;
                    }
                }
            }
        });
        Self { url, addr, seen }
    }

    pub(crate) fn requests(&self) -> Vec<Received> {
        self.seen.lock().unwrap().clone()
    }

    /// The manifest the first open carried.
    pub(crate) fn opened(&self) -> serde_json::Value {
        manifest_part(self.requests().iter().find(|r| is_open(r)).expect("the publish never opened a deploy"))
    }
}

fn answer(req: &Received, state: &Mutex<Pending>, seta: &Seta) -> Reply {
    let path = req.path_and_query.split('?').next().unwrap_or("");
    let Some((site, rest)) = path.strip_prefix("/api/sites/").and_then(|p| p.split_once('/')) else {
        return Reply::Json(404, serde_json::json!({ "error": "Not found" }));
    };
    let mut state = state.lock().unwrap();
    let state = &mut *state;
    // A deploy completes once nothing is needed; opening it again finds it live.
    let answer_for = |state: &mut Pending| {
        if state.deploy == state.live {
            return serde_json::json!({ "deploy": state.deploy, "state": "live", "needed": [], "live_generation": state.live });
        }
        if state.needed.is_empty() {
            state.live = state.deploy.clone();
            let mut live = serde_json::json!({ "deploy": state.deploy, "state": "live", "needed": [], "live_generation": state.deploy,
                "url": format!("https://{site}.example"), "files_updated": state.files, "files_removed": 0, "timestamp": 1_700_000_000 });
            for (k, v) in seta.committed.as_object().into_iter().flatten() {
                live[k] = v.clone();
            }
            return live;
        }
        let needed: Vec<_> = state.needed.iter().map(|(p, (e, s))| serde_json::json!({ "entry": e, "path": p, "size": s })).collect();
        serde_json::json!({ "deploy": state.deploy, "state": "open", "needed": needed,
            "limits": { "request_bytes": seta.request_bytes, "chunk_bytes": 50 << 20, "concurrency_hint": 6 }, "live_generation": null })
    };
    let (send_path, commit_path) = (format!("deploys/{}/files", state.deploy), format!("deploys/{}/commit", state.deploy));
    match req.method.as_str() {
        "POST" if rest == "deploys" => {
            let manifest = manifest_part(req);
            let files: std::collections::HashMap<String, String> = serde_json::from_value(manifest["files"].clone()).unwrap_or_default();
            let deploy = crate::build::assets::paths::compute_manifest_generation_id(&files);
            if seta.already_live {
                state.live = deploy.clone();
            }
            if deploy != state.deploy {
                state.deploy = deploy;
                state.files = files.len();
                state.needed = files.iter().map(|(p, e)| (p.clone(), (e.clone(), manifest["sizes"][e].as_u64().unwrap_or(0)))).collect();
            }
            take_parts(req, &mut state.needed);
            Reply::Json(200, answer_for(state))
        }
        "POST" if rest == send_path => {
            take_parts(req, &mut state.needed);
            Reply::Json(200, answer_for(state))
        }
        "POST" if rest == commit_path => Reply::Json(200, answer_for(state)),
        "PUT" if req.header("x-moss-generation") == Some(state.deploy.as_str()) => {
            let file = urlencoding::decode(rest.trim_start_matches("files/")).unwrap().into_owned();
            state.needed.remove(&file);
            Reply::Json(204, serde_json::json!({}))
        }
        "GET" if rest == "generation" => match &seta.generation {
            Some(g) => Reply::Json(200, g.clone()),
            None => Reply::Json(404, serde_json::json!({ "error": "Not found" })),
        },
        _ => Reply::Json(404, serde_json::json!({ "error": "Not found" })),
    }
}

/// The multipart parts of `req`: (name, bytes), `manifest` gunzipped.
fn parts(req: &Received) -> Vec<(String, Vec<u8>)> {
    let Some(boundary) = req.header("content-type").and_then(|c| c.split("boundary=").nth(1)) else { return Vec::new() };
    let delim = format!("--{boundary}");
    let body = &req.body;
    let mut out = Vec::new();
    let starts: Vec<usize> = body.windows(delim.len()).enumerate().filter(|(_, w)| *w == delim.as_bytes()).map(|(i, _)| i).collect();
    for pair in starts.windows(2) {
        let chunk = &body[pair[0] + delim.len()..pair[1]];
        let Some(head_end) = chunk.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
        let head = String::from_utf8_lossy(&chunk[..head_end]).to_string();
        let name = head.split("name=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("").to_string();
        let mut bytes = chunk[head_end + 4..chunk.len().saturating_sub(2)].to_vec();
        if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut inflated = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut inflated).unwrap();
            bytes = inflated;
        }
        out.push((name, bytes));
    }
    out
}

pub(crate) fn manifest_part(req: &Received) -> serde_json::Value {
    parts(req).into_iter().find(|(n, _)| n == "manifest").map(|(_, b)| serde_json::from_slice(&b).unwrap()).unwrap_or_default()
}

/// The entries of the file parts `req` carries.
fn part_entries(req: &Received) -> Vec<String> {
    parts(req).into_iter().map(|(n, _)| n).filter(|n| n != "manifest").collect()
}

fn take_parts(req: &Received, needed: &mut BTreeMap<String, (String, u64)>) {
    for entry in part_entries(req) {
        needed.retain(|_, (e, _)| *e != entry);
    }
}

pub(crate) fn is_deploy_send(r: &Received) -> bool {
    r.method == "POST" && r.path_and_query.contains("/deploys/") && r.path_and_query.ends_with("/files")
}

pub(crate) fn is_open(r: &Received) -> bool {
    r.method == "POST" && r.path_and_query.ends_with("/deploys")
}

/// A sealed generation of `files` (path -> bytes) and `links` (path ->
/// target), written to disk where the publish reads it.
fn generation(dir: &std::path::Path, files: &[(&str, &[u8])], links: &[(&str, &str)]) -> SealedManifest {
    let mut pending = PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(dir));
    for (p, bytes) in files {
        let sp = ServedPath::from_source(p.trim_start_matches("_gated/")).unwrap();
        let sp = if p.starts_with("_gated/") { sp.gated() } else { sp };
        pending.register(&sp, bytes, HashBucket::Files);
    }
    for (p, target) in links {
        pending.register_hashed(&ServedPath::from_source(p).unwrap(), &crate::types::content::symlink_entry(target), HashBucket::Files);
    }
    let sealed = pending.seal();
    let gen_dir = MossPaths::new(dir).generation_dir(sealed.generation_id());
    for (p, bytes) in files {
        let at = gen_dir.join(p);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
    }
    for (p, target) in links {
        std::fs::create_dir_all(&gen_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, gen_dir.join(p)).unwrap();
    }
    sealed
}

/// Publishes `sealed` from `dir` to the stand-in at `url`; the outcome and
/// every progress event.
async fn publish(dir: &std::path::Path, sealed: &SealedManifest, url: &str) -> (Result<PushResult, String>, Vec<progress::DeployProgress>) {
    let prev = std::env::var("MOSS_SETA_URL").ok();
    std::env::set_var("MOSS_SETA_URL", url);
    let identity = Identity::generate().expect("generate identity");
    let recording = Arc::new(RecordingSink::default());
    let sink: Arc<dyn progress::DeploySink> = recording.clone();
    let ports = crate::deploy::one_shot::HeadlessDeployPorts;
    let events_lock = Arc::new(tokio::sync::Mutex::new(()));
    let cx = PushContext { folder_path: dir, identity: &identity, site_id: SITE, sink: &sink, ports: &ports, events_lock: &events_lock };
    let result = push_site_inner(sealed, &cx).await.map_err(|e| e.to_string());
    match prev {
        Some(u) => std::env::set_var("MOSS_SETA_URL", u),
        None => std::env::remove_var("MOSS_SETA_URL"),
    }
    (result, recording.take())
}

const PAGES: &[(&str, &[u8])] = &[("index.html", b"<h1>home</h1>"), ("a/index.html", b"page a"), ("b/index.html", b"page b")];

fn assert_published(result: &Result<PushResult, String>, seen: &[Received]) {
    assert!(seen.iter().any(is_open), "the publish never opened a deploy: {result:?}");
    assert!(matches!(result, Ok(PushResult::Success { .. })), "got {result:?}");
    assert!(!seen.iter().any(|r| r.path_and_query.ends_with("/sync") || r.path_and_query.ends_with("/commit") && !r.path_and_query.contains("/deploys/")), "the old route was used");
}

/// A send cut off mid-request is sent again, freshly signed since seta accepts
/// a nonce once, and only the files the server still lacks go a second time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_send_is_resumed_and_sends_only_what_the_server_lacks() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let dropped = Arc::new(Mutex::new(false));
    let once = Arc::clone(&dropped);
    let seta = DeployStandIn::start(4 << 20, Box::new(move |r, _| {
        (is_deploy_send(r) && !std::mem::replace(&mut *once.lock().unwrap(), true)).then_some(Reply::Drop)
    })).await;
    let dir = tempfile::tempdir().unwrap();
    // Over open's 1 MiB of inline files together, so at least one goes by a send.
    let pages: Vec<(String, Vec<u8>)> = (0..3).map(|i| (format!("p{i}/index.html"), vec![b'a' + i as u8; 400 << 10])).collect();
    let pages: Vec<(&str, &[u8])> = pages.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let sealed = generation(dir.path(), &pages, &[]);

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert_published(&result, &seen);
    let at = seen.iter().position(is_deploy_send).expect("no send was made to drop");
    let mut lost = part_entries(&seen[at]);
    let mut resent: Vec<String> = seen[at + 1..].iter().filter(|r| is_open(r) || is_deploy_send(r)).flat_map(part_entries).collect();
    lost.sort();
    resent.sort();
    assert!(!lost.is_empty(), "the dropped send carried no file");
    assert_eq!(resent, lost, "after the drop the client must send exactly what the dropped send carried");
    let signed: Vec<&str> = seen.iter().filter_map(|r| r.header("authorization")).collect();
    assert_eq!(signed.len(), seen.len(), "every request is signed");
    assert_eq!(signed.iter().collect::<std::collections::HashSet<_>>().len(), signed.len(), "a retry reused a signature");
}

/// Old route: `deploy::push::tests::the_sync_sends_regular_file_sizes_and_the_upload_counts_them`.
/// The app's panel counts the needed files and bytes, and the count reaches the total.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_progress_counts_each_needed_file_and_its_bytes_to_the_total() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let sealed = generation(dir.path(), PAGES, &[]);

    let (result, events) = publish(dir.path(), &sealed, &seta.url).await;

    assert_published(&result, &seta.requests());
    let bytes: u64 = PAGES.iter().map(|(_, b)| b.len() as u64).sum();
    let last = events.iter().rev().find(|e| e.stage == DeployStage::Uploading).expect("an upload sample");
    assert_eq!((last.current, last.total), (PAGES.len() as u32, PAGES.len() as u32));
    assert_eq!((last.bytes_uploaded, last.bytes_total), (Some(bytes as f64), Some(bytes as f64)));
}

/// Old route: `deploy::push::tests::a_server_that_gates_is_sent_the_gated_files_and_their_policy`.
/// The manifest names each members-only file with its policy, as `/sync` does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_manifest_names_each_gated_file_with_its_policy_and_the_gated_bytes_are_sent() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let sealed = generation(dir.path(), &[("index.html", b"home"), ("_gated/map/rows.csv", b"rows")], &[]);

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert_published(&result, &seen);
    let manifest = manifest_part(seen.iter().find(|r| is_open(r)).unwrap());
    let files: std::collections::HashMap<String, String> = serde_json::from_value(manifest["files"].clone()).unwrap();
    assert_eq!(manifest["gated"], serde_json::json!(crate::build::emit::members::gated_entries(&files)));
    assert!(manifest["gated"].to_string().contains("_gated/map/rows.csv"), "got {manifest}");
}

/// Old route: `deploy::push::tests::the_sync_sends_regular_file_sizes_and_the_upload_counts_them`.
/// A link is never a file part: it goes by `PUT /files/<path>` as a link,
/// carrying the deploy id.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_symlink_goes_by_put_with_its_entry_mode_and_the_deploy_id() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let sealed = generation(dir.path(), &[("index.html", b"home")], &[("latest", "index.html")]);

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert_published(&result, &seen);
    let put = seen.iter().find(|r| r.method == "PUT" && r.path_and_query.ends_with("/files/latest")).expect("the link's PUT");
    assert_eq!((put.header("x-moss-entry"), put.header("x-moss-generation")), (Some("120000"), Some(sealed.generation_id())));
    assert_eq!(put.body, b"index.html");
    assert!(seen.iter().flat_map(part_entries).all(|e| !e.starts_with("120000:")), "a link went as a part");
    let manifest = seta.opened();
    let home = manifest["files"]["index.html"].as_str().unwrap();
    assert_eq!(manifest["sizes"], serde_json::json!({ home: 4 }), "sizes name regular files only, by entry");
}

/// A sealed generation of `entries` (path -> the entry the seal recorded), and
/// the generation directory, where the test writes what is really on disk.
fn sealed_with(dir: &std::path::Path, entries: &[(&str, String)]) -> (SealedManifest, std::path::PathBuf) {
    let mut pending = PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(dir));
    for (p, entry) in entries {
        pending.register_hashed(&ServedPath::from_source(p).unwrap(), entry, HashBucket::Files);
    }
    let sealed = pending.seal();
    let gen_dir = MossPaths::new(dir).generation_dir(sealed.generation_id());
    std::fs::create_dir_all(&gen_dir).unwrap();
    (sealed, gen_dir)
}

/// The files the last open named.
fn last_opened(seen: &[Received]) -> serde_json::Value {
    manifest_part(seen.iter().rev().find(|r| is_open(r)).expect("an open"))["files"].clone()
}

/// Old route: `deploy::push::tests::a_self_healed_file_corrects_its_manifest_entry_before_commit`.
/// A file a racing rebuild rewrote after the seal ships with its real bytes,
/// and the deploy that goes live names their hash, never the sealed one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_rewritten_since_the_seal_goes_live_under_its_real_hash() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let real = b"<html>what a rebuild wrote after the seal</html>";
    let stale = crate::types::content::file_entry("0000000000000000");
    let (sealed, gen_dir) = sealed_with(dir.path(), &[("index.html", stale.clone())]);
    std::fs::write(gen_dir.join("index.html"), real).unwrap();

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert_published(&result, &seen);
    let healed = crate::types::content::file_entry(&crate::build::assets::paths::compute_binary_hash(real));
    assert_eq!(last_opened(&seen), serde_json::json!({ "index.html": healed }));
    assert!(seen.iter().flat_map(part_entries).all(|e| e != stale), "the stale entry was sent");
}

/// Old route: `deploy::push::tests::a_drifted_symlink_target_self_heals_instead_of_failing_the_deploy`.
/// A link re-pointed after the seal ships with its current target.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_re_pointed_since_the_seal_goes_live_with_its_current_target() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let (sealed, gen_dir) = sealed_with(dir.path(), &[("link", crate::types::content::symlink_entry("stale-target"))]);
    std::os::unix::fs::symlink("current-target", gen_dir.join("link")).unwrap();

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert_published(&result, &seen);
    assert_eq!(last_opened(&seen), serde_json::json!({ "link": crate::types::content::symlink_entry("current-target") }));
    let put = seen.iter().find(|r| r.method == "PUT").expect("the link's PUT");
    assert_eq!(put.body, b"current-target");
}

/// Old route: `seta::sites_tests::upload_file_chunked_signs_each_request_and_sends_the_generation_once`.
/// A file over the server's `request_bytes` goes by the chunked routes, whose
/// session carries the deploy id the server named, never as a part.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_over_request_bytes_goes_chunked_with_the_deploy_id() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(64, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let big = vec![b'x'; 200];
    let sealed = generation(dir.path(), &[("index.html", b"home"), ("big.bin", &big)], &[]);

    let _ = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert!(seen.iter().any(is_open), "the publish never opened a deploy");
    let create = seen.iter().find(|r| r.method == "POST" && r.path_and_query == format!("/api/sites/{SITE}/upload")).expect("a chunked session for big.bin");
    assert_eq!(create.header("x-moss-generation"), Some(sealed.generation_id()));
    let opened = manifest_part(seen.iter().find(|r| is_open(r)).unwrap());
    let big_entry = opened["files"]["big.bin"].as_str().unwrap().to_string();
    assert!(seen.iter().flat_map(part_entries).all(|e| e != big_entry), "the big file went as a part");
}

/// seta that cannot keep members-only files private: it refuses an open that names one.
fn refuse_gated(r: &Received, _: usize) -> Option<Reply> {
    (is_open(r) && manifest_part(r).get("gated").is_some())
        .then(|| Reply::Json(422, serde_json::json!({ "error": "This server cannot keep members-only files private.", "code": "gated_not_enforceable" })))
}

/// Old route: `deploy::push::tests::a_server_that_cannot_gate_is_sent_the_public_files_only`.
/// A server that refuses members-only files is opened again with the public
/// files, a deploy of their own; once those are live the same build sends nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_cannot_gate_is_opened_again_with_the_public_files_only() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(refuse_gated)).await;
    let dir = tempfile::tempdir().unwrap();
    let sealed = generation(dir.path(), &[("index.html", b"home"), ("_gated/map/rows.csv", b"rows")], &[]);

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let seen = seta.requests();
    assert_published(&result, &seen);
    let public = last_opened(&seen);
    assert_eq!(public.as_object().unwrap().keys().collect::<Vec<_>>(), ["index.html"]);
    let gated_entry = seta.opened()["files"]["_gated/map/rows.csv"].as_str().unwrap().to_string();
    assert!(seen.iter().flat_map(part_entries).all(|e| e != gated_entry), "a members-only file was sent");

    let live = DeployStandIn::serve(Seta { already_live: true, ..Seta::default() }, Box::new(refuse_gated)).await;
    let (result, _) = publish(dir.path(), &sealed, &live.url).await;
    assert!(matches!(result, Ok(PushResult::Success { .. })), "got {result:?}");
    let seen = live.requests();
    assert_eq!(seen.iter().filter(|r| is_open(r)).count(), 2, "the gated open and the public one");
    assert!(!seen.iter().any(is_deploy_send), "nothing is sent to a site already serving the public files");
}

/// Old route: `deploy::push::tests::a_server_error_on_sync_fails_the_publish_after_one_sync`.
/// A server error says nothing about gating: the open, retried to its four
/// attempts, fails the publish, and no public-only open follows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_error_on_open_fails_the_publish_with_no_public_only_open() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|r, _| is_open(r).then(|| Reply::Json(500, serde_json::json!({}))))).await;
    let dir = tempfile::tempdir().unwrap();
    let sealed = generation(dir.path(), &[("index.html", b"home"), ("_gated/map/rows.csv", b"rows")], &[]);

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    assert!(result.is_err(), "got {result:?}");
    let opens: Vec<_> = seta.requests().into_iter().filter(is_open).collect();
    assert_eq!(opens.len(), 4, "one open, retried");
    assert!(opens.iter().all(|r| manifest_part(r).get("gated").is_some()), "a public-only open followed");
}

/// Old route: `deploy::push::tests::a_needed_file_without_a_size_fails_its_upload`.
/// A directory where the manifest names a file fails the publish with a plain
/// error before anything is opened, rather than going out as 0 bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_directory_where_the_manifest_names_a_file_fails_before_the_open() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let seta = DeployStandIn::start(4 << 20, Box::new(|_, _| None)).await;
    let dir = tempfile::tempdir().unwrap();
    let (sealed, gen_dir) = sealed_with(dir.path(), &[("index.html", crate::types::content::file_entry("0000000000000001"))]);
    std::fs::create_dir_all(gen_dir.join("index.html")).unwrap();

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    let error = result.expect_err("a directory is not a file");
    assert!(error.ends_with("Failed to stat index.html"), "{error}");
    assert!(!seta.requests().iter().any(is_open));
}

/// The answers seta gives to open that the client turns into a named refusal:
/// (status, body, the words the publish ends with). The words are what the
/// same answer to `/sync` ended with; `mass_removal` is new to open.
fn refusals() -> Vec<(u16, serde_json::Value, String)> {
    vec![
        (413, serde_json::json!({ "error": "This site is over its storage cap.", "code": "storage_cap_exceeded", "cap_bytes": 1000, "site_bytes": 900 }), "This site is over its storage cap.".into()),
        (403, serde_json::json!({ "error": "This folder is not signed in.", "code": "folder_not_signed_in", "site": "other-site" }), "This folder isn't signed in to other-site. Sign it in again to publish it.".into()),
        (403, serde_json::json!({ "error": "subscription_expired" }), "API error: 403 - subscription_expired".into()),
        (403, serde_json::json!({ "error": "Not authorized for this site" }), "API error: 403 - Not authorized for this site".into()),
        (422, serde_json::json!({ "error": "This server cannot keep members-only files private.", "code": "gated_not_enforceable" }), "This server cannot keep members-only files private.".into()),
        (410, serde_json::json!({ "error": "update", "code": "update_required", "message": "Update moss to publish.", "min_version": "0.16.0" }), "Update moss to publish. This needs moss 0.16.0 or newer.".into()),
        (409, serde_json::json!({ "error": "A newer publish replaced this one.", "code": "deploy_superseded" }), "A newer publish replaced this one.".into()),
        (409, serde_json::json!({ "error": "mass_removal", "code": "mass_removal", "files": 3, "live_files": 40, "removed": 37 }), crate::deploy::mass_remove::wipe_refusal(3, 40, 37)),
    ]
}

/// Old route: `deploy::push::tests::a_sync_that_fails_for_another_reason_sends_nothing`,
/// `seta::client_tests` (storage cap, update required, superseded,
/// sign-in) and `deploy::profile_tests::named_seta_refusals_record_their_kinds`.
/// Each refusal to open ends the publish with the words it had on `/sync`,
/// and before anything is sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_refusal_answered_to_open_ends_the_publish_with_its_old_words() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let mut wrong = Vec::new();
    for (status, body, words) in refusals() {
        let dir = tempfile::tempdir().unwrap();
        let sealed = generation(dir.path(), PAGES, &[]);
        let seta = DeployStandIn::start(4 << 20, Box::new(move |r, _| is_open(r).then(|| Reply::Json(status, body.clone())))).await;

        let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

        let seen = seta.requests();
        assert!(seen.iter().any(is_open), "{status}: the publish never opened a deploy");
        assert!(!seen.iter().any(is_deploy_send), "{status}: a refused open was followed by a send");
        assert_eq!(seen.iter().filter(|r| is_open(r)).count(), 1, "{status}: the refused open was sent again");
        if result.as_ref().err() != Some(&words) {
            wrong.push(format!("{status}: {result:?}"));
        }
    }
    assert!(wrong.is_empty(), "ended with other words:\n{}", wrong.join("\n"));
}

/// A send whose response is lost after the server staged its parts errors,
/// while a sibling's answer is live: the publish succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_answer_wins_over_a_sibling_send_that_failed() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let first: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
    let failing = Arc::clone(&first);
    let seta = DeployStandIn::start(500 << 10, Box::new(move |r, _| {
        if !is_deploy_send(r) {
            return None;
        }
        let entries = part_entries(r);
        let mut first = failing.lock().unwrap();
        (first.get_or_insert_with(|| entries.clone()) == &entries).then_some(Reply::Staged(400))
    })).await;
    let dir = tempfile::tempdir().unwrap();
    let pages: Vec<(String, Vec<u8>)> = (0..3).map(|i| (format!("p{i}/index.html"), vec![b'a' + i as u8; 400 << 10])).collect();
    let pages: Vec<(&str, &[u8])> = pages.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let sealed = generation(dir.path(), &pages, &[]);

    let (result, _) = publish(dir.path(), &sealed, &seta.url).await;

    assert!(first.lock().unwrap().is_some(), "no send was failed");
    assert_published(&result, &seta.requests());
}
