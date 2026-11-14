use super::*;
use crate::deploy::{prebuilt, push, progress};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

/// What the stub saw, to hold the record's counts against.
#[derive(Default)]
struct Seen { requests: AtomicU64, failed_put_bytes: AtomicU64 }

/// Keeps the committed manifest so a redeploy really compares identical output.
/// Every answer carries `ray` as its `cf-ray`; the first PUT is answered 503
/// when `fail_first_put`.
async fn mock_seta(ray: Option<&'static str>, fail_first_put: bool)
    -> (std::net::SocketAddr, tokio::task::JoinHandle<()>, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let seen_in_task = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut live = serde_json::Map::new();
        let mut failed_once = !fail_first_put;
        let seen = seen_in_task;
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let raw = crate::test_drain_http_request(&mut stream).await;
            let head_end = raw.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
            let head = String::from_utf8_lossy(&raw[..head_end]);
            let line = head.lines().next().unwrap();
            seen.requests.fetch_add(1, Ordering::Relaxed);
            let (status, body) = if line.starts_with("PUT ") && !failed_once {
                failed_once = true;
                seen.failed_put_bytes.store((raw.len() - head_end) as u64, Ordering::Relaxed);
                ("503 Service Unavailable", json!({}))
            } else if line.contains("/sync ") {
                let input: Value = serde_json::from_slice(&raw[head_end..]).unwrap();
                let files = input["files"].as_object().unwrap();
                let need: Vec<_> = files.iter().filter(|(k, v)| live.get(*k) != Some(*v)).map(|(k, _)| k).collect();
                ("200 OK", json!({"need": need, "remove": []}))
            } else if line.contains("/commit ") {
                let input: Value = serde_json::from_slice(&raw[head_end..]).unwrap();
                live = input["manifest"].as_object().unwrap().clone();
                ("200 OK", json!({"url": "https://profile-test.mosspub.com", "files_updated": live.len(), "files_removed": 0, "timestamp": 1700000000}))
            } else if line.starts_with("PUT ") {
                ("200 OK", json!({}))
            } else {
                // Force the /sync comparison even when generation IDs match.
                ("404 Not Found", json!({}))
            };
            let body = body.to_string();
            let ray = ray.map(|r| format!("cf-ray: {r}\r\n")).unwrap_or_default();
            let response = format!("HTTP/1.1 {status}\r\n{ray}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            crate::test_respond_and_close(&mut stream, response.as_bytes()).await;
        }
    });
    (addr, task, seen)
}

fn records(folder: &Path) -> Vec<Value> {
    std::fs::read_to_string(crate::moss_paths::MossPaths::new(folder).profile())
        .expect("deploy appends profile.jsonl").lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("one JSON object per line"))
        .collect()
}

async fn publish_twice(prebuilt: bool) {
    let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".moss")).unwrap();
    std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"profile-test\"\n").unwrap();
    crate::identity::Identity::generate().unwrap().save(dir.path()).unwrap();
    let output = dir.path().join("output");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("index.html"), "<html>private-page-content</html>").unwrap();
    std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\nuid: profile-home\n---\n\nprivate-page-content\n").unwrap();
    let (addr, server, _) = mock_seta(None, false).await;
    let previous = std::env::var("MOSS_SETA_URL").ok();
    std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
    let sink = progress::silent();
    for run in 1..=2 {
        let result = if prebuilt {
            prebuilt::run_prebuilt_deploy(dir.path(), &output, None, false, false, &sink).await
        } else {
            push::run_hosted_deploy(dir.path(), &crate::cli::host::cli_host_ports,
                crate::build::PluginMode::Skip, None, push::PublishOverrides::default(), &sink).await
        };
        assert!(matches!(result, Ok(PushResult::Success { .. })), "{result:?}");
        let all = records(dir.path());
        let deploys: Vec<_> = all.iter().filter(|r| r["kind"] == "deploy").collect();
        assert_eq!(deploys.len(), run, "exactly one deploy record per invocation");
        let record = deploys.last().unwrap();
        assert_eq!(record["schema"], 1);
        assert_eq!(record["outcome"], "ok");
        assert_eq!(record["target"], if prebuilt { "prebuilt" } else { "seta" });
        assert_eq!(record["dry_run"], false);
        assert_eq!(record["build_profile"], crate::system::build_profile());
        let c = &record["counts"];
        let needed = c["files_needed"].as_u64().unwrap();
        assert!(needed <= c["manifest_files"].as_u64().unwrap());
        assert_eq!(c["files_uploaded"], needed);
        for phase in ["build", "local_checks", "manifest", "sync", "upload", "publish", "after_publish", "history_snapshot"] {
            assert!(record["phases"].as_array().unwrap().iter().any(|p| p["name"] == phase && p["ms"].is_number()));
        }
        if run == 1 {
            assert!(needed > 0);
            assert!(c["bytes_uploaded"].as_u64().unwrap() > 0);
            assert_eq!(c["body_requests"], needed);
            assert_eq!(c["single_put_files"], needed);
            assert_eq!(c["server_files_updated"], c["manifest_files"]);
        } else {
            assert_eq!(needed, 0, "unchanged output needs no upload");
            assert_eq!(c["body_requests"], 0);
            assert_eq!(c["bytes_uploaded"], 0);
            let upload = record["phases"].as_array().unwrap().iter().find(|p| p["name"] == "upload").unwrap();
            assert_eq!(upload["ms"], 0, "no upload span was entered");
            assert_eq!((c["sent_kb"].as_u64(), c["resent_kb"].as_u64(), c["requests_failed"].as_u64()), (Some(0), Some(0), Some(0)));
            assert!(c["requests"].as_u64().unwrap() >= 1, "the generation probe and sync were counted");
            assert_eq!(record["edge"], "none", "an answer without cf-ray has no edge");
        }
        if !prebuilt {
            assert!(all.iter().any(|b| b["kind"] == "build" && b["started_at"] == record["build_started_at"]));
        }
        let encoded = record.to_string();
        for secret in ["http", "private-page-content", "index.html", "profile-test", "Authorization"] {
            assert!(!encoded.contains(secret), "profile leaked {secret}");
        }
    }
    server.abort();
    match previous { Some(v) => std::env::set_var("MOSS_SETA_URL", v), None => std::env::remove_var("MOSS_SETA_URL") }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deploy_profile_hosted_fresh_and_unchanged() { publish_twice(false).await; }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deploy_profile_prebuilt_fresh_and_unchanged() { publish_twice(true).await; }

#[tokio::test]
async fn deploy_profile_refusal_records_once_without_error_text() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    crate::system::build_records::records().install_publish_preflight(
        folder.to_str().unwrap(), crate::build::types::PublishPreflightProjection {
            build_generation: 1, missing_references: vec![],
            unresolved_inputs: vec!["private-note.md".into()], drafts_linked: vec![],
        },
    );
    let sealed = crate::build::manifest::PendingManifest::for_build(
        crate::types::content::SiteHashes::default(),
        crate::build::cloud_ledger::InputEvidence::new(folder),
    ).seal();
    let identity = crate::identity::Identity::generate().unwrap();
    let sink = progress::silent();
    let events_lock = Arc::new(tokio::sync::Mutex::new(()));
    let cx = push::PushContext {
        folder_path: folder, identity: &identity, site_id: "profile-test", sink: &sink,
        ports: &crate::deploy::one_shot::HeadlessDeployPorts, events_lock: &events_lock,
    };
    // The CLI scope and the inner publish must share a single append.
    let result = record(folder, "seta", false, push::push_site_inner(&sealed, &cx)).await;
    assert!(result.unwrap_err().to_string().contains("private-note.md"));
    let rows = records(folder);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "deploy");
    assert_eq!(rows[0]["outcome"], "refused");
    assert_eq!(rows[0]["refusal_kind"], "unresolved_inputs");
    assert!(!rows[0].to_string().contains("private-note"));
}

#[tokio::test]
async fn deploy_profile_prebuilt_error_is_recorded() {
    let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let identity = crate::identity::Identity::generate().unwrap();
    let result = prebuilt::push_prebuilt(dir.path(), &dir.path().join("missing"),
        "profile-test", &identity, false, &progress::silent()).await;
    assert!(result.is_err());
    let rows = records(dir.path());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["outcome"], "error");
    assert_eq!(rows[0]["target"], "prebuilt");
    assert_eq!(rows[0]["counts"]["files_uploaded"], 0);
    let c = &rows[0]["counts"];
    for key in ["requests", "requests_failed", "sent_kb", "resent_kb", "manifest_kb"] {
        assert_eq!(c[key], 0, "{key} is declared even when no request was made");
    }
    assert_eq!(rows[0]["edge"], "none");
}

/// A key is a count of digits no longer than a terabyte in KB, a unit, or a word.
fn longest_digit_run(text: &str) -> usize {
    text.split(|c: char| !c.is_ascii_digit()).map(str::len).max().unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deploy_profile_counts_requests_resends_and_edge_without_leaking_inputs() {
    let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("output");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("index.html"), "<html>private-page-content</html>").unwrap();
    let ray = "12345678901234567890123-FRA";
    let (addr, server, seen) = mock_seta(Some(ray), true).await;
    let previous = std::env::var("MOSS_SETA_URL").ok();
    std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
    let identity = crate::identity::Identity::generate().unwrap();
    let site_id = "../evil/host.example.com-1234567890123456";
    let result = prebuilt::push_prebuilt(dir.path(), &output, site_id, &identity, false, &progress::silent()).await;
    assert!(matches!(result, Ok(PushResult::Success { .. })), "{result:?}");
    server.abort();
    match previous { Some(v) => std::env::set_var("MOSS_SETA_URL", v), None => std::env::remove_var("MOSS_SETA_URL") }

    let rows = records(dir.path());
    let record = rows.iter().find(|r| r["kind"] == "deploy").unwrap();
    let c = &record["counts"];
    assert_eq!(c["requests"], seen.requests.load(Ordering::Relaxed), "every request the server saw was counted");
    assert_eq!(c["requests_failed"], 1);
    let lost = seen.failed_put_bytes.load(Ordering::Relaxed);
    assert!(lost > 0);
    assert_eq!(c["resent_kb"], lost.div_ceil(1024));
    assert_eq!(c["sent_kb"], lost.div_ceil(1024), "the retry carried the same body");
    assert_eq!(record["edge"], "FRA");
    let line = record.to_string();
    assert!(!line.contains('/') && !line.contains('@'), "{line}");
    assert!(longest_digit_run(&line) < 13, "{line}");
    for secret in [ray, "evil", "example", "private-page-content", "index.html"] {
        assert!(!line.contains(secret), "line leaked {secret}: {line}");
    }
    println!("{line}");
}

#[test]
fn edge_is_the_ray_suffix_when_it_is_a_location_code() {
    use crate::seta::wire::WireCounters;
    let edge_of = |ray: Option<&str>| {
        let wire = WireCounters::default();
        wire.note(0, Some(200), ray);
        wire.edge().to_string()
    };
    assert_eq!(edge_of(Some("8f2a1b3c4d5e6f70-FRA")), "FRA");
    assert_eq!(edge_of(Some("8f2a-fra")), "FRA");
    assert_eq!(edge_of(None), "none");
    for bad in ["8f2a1b3c4d5e6f70", "8f2a-", "8f2a-FR", "8f2a-FRANK", "8f2a-F/A", "8f2a-12A", "8f2a-FRA\u{e9}"] {
        assert_eq!(edge_of(Some(bad)), "none", "{bad}");
    }
    let wire = WireCounters::default();
    wire.note(0, Some(200), Some("a-FRA"));
    wire.note(0, Some(200), Some("a-AMS"));
    assert_eq!(wire.edge(), "FRA", "the first answer with a ray decides");
}

/// The server's named refusals are recorded under the kinds their bare
/// statuses already had.
#[tokio::test]
async fn named_seta_refusals_record_their_kinds() {
    use crate::seta::client::SetaError;
    let kind_of = |error: SetaError| {
        let slot = Arc::new(Mutex::new(None));
        REFUSAL.sync_scope(slot.clone(), || seta_error(&error));
        let kind = slot.lock().unwrap().clone();
        kind
    };
    let cap = SetaError::StorageCapExceeded { cap_bytes: 1, site_bytes: None, message: "over".into() };
    assert_eq!(kind_of(cap), Some(json!("size_limit")));
    assert_eq!(kind_of(SetaError::DeploySuperseded { message: "again".into() }), Some(json!("conflict")));
}
