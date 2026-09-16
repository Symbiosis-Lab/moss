use super::*;

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

/// Serializes every test in THIS binary that touches the publish latch.
///
/// `moss_build::deploy::freeze` declares its own twin, and that is not
/// duplication to fold: a `#[cfg(test)]` static compiles only into its own
/// crate's test binary, so the two are never live in the same process.
static PUBLISH_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    let err = push_prebuilt(project.path(), &missing, "guard-test-site", &identity, &silent())
        .await
        .expect_err("a prebuilt publish must be rejected while a publish is in flight");
    assert_eq!(
        err, PUBLISH_IN_PROGRESS_MSG,
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

    let err = push_prebuilt(project.path(), &missing, "guard-test-site", &identity, &silent())
        .await
        .expect_err("a version-ahead config must refuse");
    assert!(err.contains("schema_version") && err.contains("newer"), "got: {err}");
}
