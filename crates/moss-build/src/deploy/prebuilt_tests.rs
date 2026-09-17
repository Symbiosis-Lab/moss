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

// ── Self-heal wiring: this call site, not the shared function ───────────────
//
// `upload_regular_file` (deploy/upload.rs) is proven to self-heal and to
// return the actually-shipped hash by upload_tests.rs's own tests, called
// directly with a stale hash handed in as a literal. None of that proves
// `push_prebuilt_inner` (this file) does the right thing with what comes
// back: `commit_sync` sends `manifest` verbatim as the server's new source of
// truth, so a healed hash that never reaches `manifest` commits a permanently
// wrong record for a file that was, in fact, uploaded correctly.
// `push.rs`'s `a_self_healed_file_corrects_its_manifest_entry_before_commit`
// proves this for the moss-format publish path; this is prebuilt's own,
// independent fold-in loop (the `self_heal_corrections` block above
// `push_prebuilt_inner`'s commit_sync call) and needs its own proof.
//
// Unlike `push.rs`, there is no separately-sealed manifest to hand a stale
// hash to — `build_manifest_from_dir` hashes the file at call time, so the
// drift has to be a real one: the mock server withholds its `sync_manifest`
// response until the file has been rewritten on disk, which deterministically
// places the rewrite between the manifest hash (already computed by then) and
// the upload's own re-read (which only happens once that response arrives).

/// Drain one raw HTTP/1.1 request off `stream`, without answering it —
/// `crate::test_mock_http_conn` drains-then-responds as one unit, which
/// cannot fit a caller that needs to do work (here: rewrite a file)
/// in between receiving a request and answering it.
async fn drain_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(hdr_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let hdr_str = String::from_utf8_lossy(&raw[..hdr_end]);
            let body_len = hdr_str
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let expected_total = hdr_end + 4 + body_len;
            while raw.len() < expected_total {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }
            break;
        }
    }
    raw
}

/// Write a response and close — the second half of what
/// `crate::test_mock_http_conn` does in one call; split out so a caller can
/// act between [`drain_request`] and this.
async fn respond(stream: &mut tokio::net::TcpStream, resp: &[u8]) {
    use tokio::io::AsyncWriteExt;
    stream.write_all(resp).await.ok();
    stream.shutdown().await.ok();
}

#[tokio::test]
async fn a_self_healed_prebuilt_file_corrects_its_manifest_entry_before_commit() {
    use sha2::{Digest, Sha256};
    use tokio::net::TcpListener;

    let _lock = PUBLISH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev_url = std::env::var("MOSS_SETA_URL").ok();

    let old_bytes = b"<html>content the manifest hash was computed from</html>";
    let new_bytes: &[u8] =
        b"<html>content actually on disk by upload time - a racing rebuild</html>";
    let hash_of = |b: &[u8]| -> String {
        let mut h = Sha256::new();
        h.update(b);
        hex::encode(h.finalize())
    };
    let old_hash = hash_of(old_bytes);
    let new_hash = hash_of(new_bytes);
    assert_ne!(old_hash, new_hash, "fixture sanity: the drift must be real");

    let project = tmp_dir();
    let prebuilt = tmp_dir();
    let target = prebuilt.path().join("index.html");
    std::fs::write(&target, old_bytes).expect("seed prebuilt file");

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (commit_tx, commit_rx) = tokio::sync::oneshot::channel();
    let rewrite_target = target.clone();
    let new_bytes_owned = new_bytes.to_vec();

    tokio::spawn(async move {
        // conn 0: GET /api/sites/:id/generation -> 404 (get_live_generation
        // short-circuit: no live generation known, proceed normally).
        {
            let (mut stream, _) = listener.accept().await.unwrap();
            drain_request(&mut stream).await;
            respond(
                &mut stream,
                b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            )
            .await;
        }
        // conn 1: POST /api/sites/:id/sync — drain the request (built from
        // OLD bytes), THEN rewrite the file on disk, THEN answer. Everything
        // downstream of this response (the file-size stat, the upload's own
        // read) happens only after the rewrite has landed.
        {
            let (mut stream, _) = listener.accept().await.unwrap();
            drain_request(&mut stream).await;
            tokio::fs::write(&rewrite_target, &new_bytes_owned)
                .await
                .expect("rewrite mid-deploy");
            respond(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 35\r\n\r\n{\"need\":[\"index.html\"],\"remove\":[]}",
            )
            .await;
        }
        // conn 2: PUT /api/sites/:id/files/index.html — the upload itself.
        {
            let (mut stream, _) = listener.accept().await.unwrap();
            drain_request(&mut stream).await;
            respond(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            )
            .await;
        }
        // conn 3: POST /api/sites/:id/commit — capture the body so the test
        // can inspect which hash actually got committed.
        {
            let (mut stream, _) = listener.accept().await.unwrap();
            let raw = drain_request(&mut stream).await;
            respond(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 104\r\n\r\n{\"url\":\"https://healed-prebuilt.mosspub.com\",\"files_updated\":1,\"files_removed\":0,\"timestamp\":1700000000}",
            )
            .await;
            commit_tx.send(raw).ok();
        }
    });

    std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
    let identity = Identity::generate().expect("generate identity");

    let result = push_prebuilt(
        project.path(),
        prebuilt.path(),
        "healed-prebuilt",
        &identity,
        &silent(),
    )
    .await;

    match prev_url {
        Some(u) => std::env::set_var("MOSS_SETA_URL", u),
        None => std::env::remove_var("MOSS_SETA_URL"),
    }

    assert!(
        result.is_ok(),
        "a hash drift stable across the settle pause must self-heal a prebuilt deploy too: {result:?}"
    );

    let commit_request = commit_rx
        .await
        .expect("commit_sync must have been called for the publish to succeed");
    let request_str = String::from_utf8_lossy(&commit_request);
    let body = request_str
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or(&request_str);
    assert!(
        body.contains(&new_hash),
        "commit_sync's manifest must carry the hash actually shipped for the self-healed file, \
         not the one the manifest was built with before the drift: {body}"
    );
    assert!(
        !body.contains(&old_hash),
        "the stale pre-drift hash must not survive into the committed manifest: {body}"
    );
}
