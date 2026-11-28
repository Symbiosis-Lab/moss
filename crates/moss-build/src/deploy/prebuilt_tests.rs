use super::*;
use crate::build::assets::paths::compute_manifest_generation_id;

/// Verify that the generation_id derived from a prebuilt manifest is a real
/// content-derived hash, not the old placeholder literal `"prebuilt"`.
///
/// This test exercises the exact code path that C1 fixes: manifest produced
/// by `build_manifest_from_dir` → `compute_manifest_generation_id` → result
/// is 16 lowercase hex chars and is NOT the literal `"prebuilt"`.
#[test]
fn prebuilt_generation_id_is_real_hash_not_placeholder() {
    // Build a temp dir with one file so build_manifest_from_dir succeeds.
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).expect("failed to create test-tmp dir");
    std::fs::write(tmp.path().join("index.html"), b"<html></html>").expect("write test file");

    let manifest = build_manifest_from_dir(tmp.path()).expect("build_manifest_from_dir");
    assert!(!manifest.is_empty(), "manifest should not be empty");

    let generation_id = compute_manifest_generation_id(&manifest);

    // Must not be the placeholder.
    assert_ne!(
        generation_id, "prebuilt",
        "generation_id must not be the old placeholder literal"
    );
    // Must be exactly 16 lowercase hex chars (xxh3_64 output).
    assert_eq!(
        generation_id.len(),
        16,
        "generation_id must be exactly 16 hex chars, got: {generation_id}"
    );
    assert!(
        generation_id.chars().all(|c| c.is_ascii_hexdigit()),
        "generation_id must be lowercase hex, got: {generation_id}"
    );
}

/// A prebuilt manifest and a normal build's sealed manifest are compared by the
/// server as opaque strings, so for the same bytes the entry must be identical
/// down to the hex formatting. A different digest marks every file as changed
/// each time a site switches between the two paths.
#[test]
fn a_prebuilt_manifest_entry_equals_the_normal_builds_entry_for_the_same_bytes() {
    let tmp = tmp_dir();
    // The second file spans several streaming reads.
    let big: Vec<u8> = (0u32..40_000).flat_map(|i| i.to_le_bytes()).collect();
    let files: [(&str, &[u8]); 3] =
        [("index.html", b"<html></html>"), ("empty.txt", b""), ("assets/big.bin", &big)];
    for (name, bytes) in files {
        let path = tmp.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    let manifest = build_manifest_from_dir(tmp.path()).expect("build_manifest_from_dir");

    for (name, bytes) in files {
        let normal_build_entry = crate::types::content::file_entry(
            &crate::build::assets::paths::compute_binary_hash(bytes),
        );
        assert_eq!(manifest[name], normal_build_entry, "{name}");
    }
}

/// Same manifest content produced in different ways (e.g. different HashMap
/// insertion order) must yield the same generation_id.
#[test]
fn prebuilt_generation_id_is_deterministic() {
    let mut m1 = HashMap::new();
    m1.insert("index.html".to_string(), "100644:aabbcc".to_string());
    m1.insert("style.css".to_string(), "100644:ddeeff".to_string());

    let mut m2 = HashMap::new();
    // Insert in reverse order — HashMap doesn't preserve insertion order.
    m2.insert("style.css".to_string(), "100644:ddeeff".to_string());
    m2.insert("index.html".to_string(), "100644:aabbcc".to_string());

    assert_eq!(
        compute_manifest_generation_id(&m1),
        compute_manifest_generation_id(&m2),
        "same manifest content must yield same generation_id regardless of insertion order"
    );
}

// ---- single-flight: prebuilt shares ONE latch with the normal publish ----
//
// `push_prebuilt` runs its own full upload loop. If it doesn't take the same
// latch `push_site` takes, `moss deploy --prebuilt` can run a second complete
// deploy on top of an in-flight one — the exact double-deploy these tests exist
// to prevent. Both directions are asserted: prebuilt must be refused while a
// normal publish holds the latch, and must itself hold it while it runs.

use crate::deploy::freeze::{with_publish_guard, PUBLISH_IN_PROGRESS_MSG};
use crate::deploy::progress::silent;

// Serializes every test in THIS binary that touches the publish latch — the
// very lock `deploy::freeze`'s own latch tests take. The latch is one
// process-global, so a lock per test module is no lock: these tests and
// freeze's run in the same test binary, and a freeze test parking a publish
// made `push_prebuilt` here answer "a publish is already running".
//
// A test that also sets `MOSS_SETA_URL` takes `crate::ENV_TEST_MUTEX` too,
// always second, so the two can never be held in opposite orders. This file's
// own lock used to guard the env var as well, while `deploy::push`'s tests
// guard it with `ENV_TEST_MUTEX`: two locks, one variable, so a prebuilt test
// and a push test running together each pointed the other's client at its
// own mock server.
use crate::deploy::freeze::single_flight_tests::TEST_LOCK as PUBLISH_TEST_LOCK;

fn tmp_dir() -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    tempfile::TempDir::new_in(&base).expect("failed to create test-tmp dir")
}

fn ok() -> Result<PushResult, String> {
    Ok(PushResult::Success {
        url: "https://x.mosspub.com".to_string(),
        files_uploaded: 1,
        files_removed: 0,
    })
}

#[tokio::test]
async fn prebuilt_publish_is_rejected_while_a_normal_publish_is_in_flight() {
    let _lock = PUBLISH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();

    let sink = silent();
    let normal = tokio::spawn(async move {
        with_publish_guard(&sink, async move {
            started_tx.send(()).ok();
            finish_rx.await.ok();
            ok()
        })
        .await
    });
    started_rx.await.expect("normal publish should start");

    let identity = Identity::generate().expect("generate identity");
    let project = tmp_dir();
    // Deliberately nonexistent: if the guard is missing, push_prebuilt gets as
    // far as its own directory check and reports THAT instead of the rejection.
    // Guarded, it never touches the filesystem or the network at all.
    let missing = project.path().join("no-such-prebuilt-dir");

    let err = push_prebuilt(project.path(), &missing, "guard-test-site", &identity, false, &silent())
        .await
        .expect_err("a prebuilt publish must be rejected while a publish is in flight");
    assert_eq!(
        err.to_string(), PUBLISH_IN_PROGRESS_MSG,
        "prebuilt publish must be refused by the single-flight latch"
    );

    finish_tx.send(()).ok();
    let done = normal.await.expect("normal publish task should not panic");
    done.expect("normal publish should succeed");
}

#[tokio::test]
async fn a_normal_publish_is_rejected_while_a_prebuilt_publish_is_in_flight() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let _lock = PUBLISH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    // Park the in-flight prebuilt publish on a connect to a dead local port:
    // it reaches its first await and stays there, and no traffic leaves the box.
    let prev_url = std::env::var("MOSS_SETA_URL").ok();
    std::env::set_var("MOSS_SETA_URL", "http://127.0.0.1:1");

    let identity = Identity::generate().expect("generate identity");
    let project = tmp_dir();
    std::fs::write(project.path().join("index.html"), b"<html></html>").expect("write file");

    let sink = silent();
    let mut in_flight = Box::pin(push_prebuilt(
        project.path(),
        project.path(),
        "guard-test-site",
        &identity,
        false,
        &sink,
    ));
    // One poll runs the whole synchronous prologue (hash the dir, resolve the
    // environment) and suspends on the first network await — the latch is held
    // from here until the future is dropped.
    let mut cx = Context::from_waker(Waker::noop());
    match in_flight.as_mut().poll(&mut cx) {
        Poll::Pending => {}
        Poll::Ready(r) => panic!("prebuilt publish finished before parking: {r:?}"),
    }

    match prev_url {
        Some(u) => std::env::set_var("MOSS_SETA_URL", u),
        None => std::env::remove_var("MOSS_SETA_URL"),
    }

    let rejected = with_publish_guard(&silent(), async { ok() }).await;
    assert_eq!(
        rejected.expect_err("a normal publish must be rejected while a prebuilt one runs"),
        PUBLISH_IN_PROGRESS_MSG,
        "push_prebuilt must HOLD the same latch, not merely check it"
    );

    drop(in_flight);
    assert!(
        with_publish_guard(&silent(), async { ok() }).await.is_ok(),
        "an abandoned prebuilt publish must not leave the latch held"
    );
}

/// A version-ahead config refuses before the directory hash — this is the
/// prebuilt route's half of the three-door guard (`push_site_inner_impl` for
/// hosted, `run_plugin_deploy_inner` for plugin, this for prebuilt), all
/// three calling `site_config::ensure_config_current`. The prebuilt dir is
/// deliberately nonexistent — same trick as
/// `prebuilt_publish_is_rejected_while_a_normal_publish_is_in_flight` above —
/// so a refusal that reached `build_manifest_from_dir` would report THAT
/// error instead, not the schema_version one. Ablated by deleting the
/// `ensure_config_current(...)?;` call at the top of `push_prebuilt_inner`:
/// goes red on "no such prebuilt dir" instead.
#[tokio::test]
async fn a_version_ahead_config_refuses_before_hashing_the_directory() {
    let _lock = PUBLISH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let identity = Identity::generate().expect("generate identity");
    let project = tmp_dir();
    let moss_dir = project.path().join(".moss");
    std::fs::create_dir_all(&moss_dir).unwrap();
    let future = crate::config::migrations::CURRENT_VERSION + 1;
    std::fs::write(moss_dir.join("config.toml"), format!("schema_version = {future}\n")).unwrap();
    let missing = project.path().join("no-such-prebuilt-dir");

    let err = push_prebuilt(project.path(), &missing, "guard-test-site", &identity, false, &silent())
        .await
        .expect_err("a version-ahead config must refuse");
    let err = err.to_string();
    assert!(err.contains("schema_version") && err.contains("newer"), "got: {err}");
}

// ── A site over its storage cap is refused at the open ──────────────────────

/// The open carries each file's size, and a server that refuses the site as
/// over its storage cap stops the publish before any upload with its own
/// sentence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_prebuilt_open_sends_sizes_and_shows_the_storage_cap_refusal() {
    use crate::deploy::push::deploy_resource_tests::{is_deploy_send, is_open, DeployStandIn, Reply};
    let _lock = PUBLISH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let prev_url = std::env::var("MOSS_SETA_URL").ok();

    let project = tmp_dir();
    let prebuilt = tmp_dir();
    let page = b"<html>a page</html>";
    std::fs::write(prebuilt.path().join("index.html"), page).unwrap();

    const SENTENCE: &str = "This folder is over the 1 GB limit. Remove some files and try again.";
    let seta = DeployStandIn::start(4 << 20, Box::new(|r, _| {
        is_open(r).then(|| Reply::Json(413, serde_json::json!({ "code": "storage_cap_exceeded", "error": SENTENCE, "cap_bytes": 1073741824u64, "site_bytes": 1586736868u64 })))
    }))
    .await;

    std::env::set_var("MOSS_SETA_URL", &seta.url);
    let identity = Identity::generate().expect("generate identity");
    let result = push_prebuilt(project.path(), prebuilt.path(), "capped-site", &identity, false, &silent()).await;
    match prev_url {
        Some(u) => std::env::set_var("MOSS_SETA_URL", u),
        None => std::env::remove_var("MOSS_SETA_URL"),
    }

    assert_eq!(result.err().map(|e| e.to_string()).as_deref(), Some(SENTENCE));
    assert!(!seta.requests().iter().any(is_deploy_send));
    let manifest = seta.opened();
    let entry = manifest["files"]["index.html"].as_str().unwrap();
    assert_eq!(manifest["sizes"], serde_json::json!({ entry: page.len() }));
}

// ── Removal gate: an upload may not take a served address offline ───────────

const GATE_SITE: &str = "gate-site";

fn record_served(project: &Path, served: &[&str]) {
    let snap = crate::build::manifest::change_set::PublishedSnapshot {
        generation_id: "g1".into(),
        target: format!("moss:{GATE_SITE}"),
        published_at: "2026-01-01T00:00:00Z".into(),
        files: served.iter().map(|p| (p.to_string(), "100644:aa".to_string())).collect(),
        ..Default::default()
    };
    crate::build::manifest::published_record::save(&crate::moss_paths::MossPaths::new(project), &snap).unwrap();
}

/// Uploads `files` against a dead local server, so a call that passes the gate
/// fails on the connection instead of reaching anything real.
async fn gated_push(project: &Path, files: &[&str], accept_removals: bool) -> Result<PushResult, String> {
    let prebuilt = tmp_dir();
    for f in files {
        let path = prebuilt.path().join(f);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    let _lock = PUBLISH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let prev_url = std::env::var("MOSS_SETA_URL").ok();
    std::env::set_var("MOSS_SETA_URL", "http://127.0.0.1:1");
    let identity = Identity::generate().expect("generate identity");
    let result = push_prebuilt(project, prebuilt.path(), GATE_SITE, &identity, accept_removals, &silent()).await.map_err(|e| e.to_string());
    match prev_url {
        Some(u) => std::env::set_var("MOSS_SETA_URL", u),
        None => std::env::remove_var("MOSS_SETA_URL"),
    }
    result
}

#[tokio::test]
async fn a_prebuilt_upload_missing_a_served_address_is_refused() {
    let project = tmp_dir();
    record_served(project.path(), &["index.html", "old/index.html"]);

    let err = gated_push(project.path(), &["index.html"], false).await.unwrap_err();

    assert!(err.starts_with("Nothing published"), "{err}");
    assert!(err.contains("/old/"), "{err}");
    assert!(err.contains("--accept-removals"), "{err}");
}

#[tokio::test]
async fn accepting_removals_lets_a_prebuilt_upload_past_the_gate() {
    let project = tmp_dir();
    record_served(project.path(), &["index.html", "old/index.html"]);

    let err = gated_push(project.path(), &["index.html"], true).await.unwrap_err();

    assert!(!err.contains("Nothing published"), "{err}");
}

#[tokio::test]
async fn a_prebuilt_upload_with_no_publish_record_is_not_gated() {
    let project = tmp_dir();

    let err = gated_push(project.path(), &["index.html"], false).await.unwrap_err();

    assert!(!err.contains("Nothing published"), "{err}");
}

#[tokio::test]
async fn only_moss_internal_hashed_files_differing_is_not_a_removal() {
    let project = tmp_dir();
    record_served(project.path(), &["index.html", "_moss/app.1111.css"]);

    let err = gated_push(project.path(), &["index.html", "_moss/app.2222.css"], false).await.unwrap_err();

    assert!(!err.contains("Nothing published"), "{err}");
}

/// A prebuilt tree's `.well-known/` files are listed for upload like any other
/// file; only the build's own `.moss-*` sidecars are not addresses.
#[test]
fn a_prebuilt_manifest_lists_files_under_well_known() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    std::fs::create_dir_all(tmp.path().join(".well-known")).expect("create .well-known");
    std::fs::write(tmp.path().join(".well-known/atproto-did"), b"did:plc:abcdefghijklmnopqrstuvwx\n").expect("write did");
    std::fs::write(tmp.path().join("index.html"), b"<html></html>").expect("write page");

    let manifest = build_manifest_from_dir(tmp.path()).expect("build_manifest_from_dir");
    assert!(manifest.contains_key(".well-known/atproto-did"), "{:?}", manifest.keys().collect::<Vec<_>>());
}

#[test]
fn a_prebuilt_folder_sends_nothing_under_the_members_only_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    for (path, body) in [("index.html", "home"), ("post/index.html", "public"), ("_gated/post/index.html", "members")] {
        let full = tmp.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, body).unwrap();
    }
    let mut paths: Vec<_> = build_manifest_from_dir(tmp.path()).unwrap().into_keys().collect();
    paths.sort();
    assert_eq!(paths, ["index.html", "post/index.html"]);
}
