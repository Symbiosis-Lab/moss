//! Tests for the chunked upload loop.
//!
//! Moved here from `sites_tests.rs` when the loop moved out of `sites.rs`, so
//! the tests sit beside the code they pin.

use super::*;

/// End-to-end test: upload_file_chunked sends X-Moss-Content-Hash on the
/// complete POST, equal to sha256 of the full file bytes.
///
/// Uses a 4-connection TcpListener mock:
///   conn 0 → GET  /uploads       → 200 [] (nothing staged; upload from zero)
///   conn 1 → POST /upload        → 200 {"uploadId":"test-upload-id"}
///   conn 2 → PATCH /upload/...   → 200 (chunk accepted)
///   conn 3 → POST /upload/.../complete → 200; we capture the request
///             headers to verify X-Moss-Content-Hash.
///
/// The file is 3 bytes (well below any planned request size) so a single chunk covers
/// it — the streaming hash and one-shot hash are identical by construction.
#[tokio::test]
async fn upload_file_chunked_sends_content_hash_header() {
    use sha2::{Digest as _, Sha256};
    use tokio::net::TcpListener;

    // ── mock server: handle exactly 3 connections ──────────────────────
    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    // Channel to receive the complete-POST request bytes from the mock.
    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();

    tokio::spawn(async move {
        /// Drain headers (up to blank line) + body per Content-Length,
        /// then write back `resp`. Handles large auth headers gracefully.
        async fn handle_conn(mut stream: tokio::net::TcpStream, resp: &[u8]) -> Vec<u8> {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut raw = Vec::new();
            // Read until we see the end of headers (\r\n\r\n) plus body.
            let mut buf = [0u8; 8192];
            loop {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                // Look for end-of-headers marker.
                if let Some(hdr_end) = find_header_end(&raw) {
                    // Parse Content-Length if present to drain body.
                    let hdr_str = String::from_utf8_lossy(&raw[..hdr_end]);
                    let body_len = hdr_str
                        .lines()
                        .find_map(|l| {
                            let low = l.to_ascii_lowercase();
                            if low.starts_with("content-length:") {
                                l.split(':')
                                    .nth(1)
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    let expected_total = hdr_end + 4 + body_len; // hdr + \r\n\r\n + body
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
            stream.write_all(resp).await.ok();
            stream.shutdown().await.ok();
            raw
        }

        fn find_header_end(data: &[u8]) -> Option<usize> {
            data.windows(4).position(|w| w == b"\r\n\r\n")
        }

        // conn 0: GET /uploads → 200 [] (nothing staged; upload from zero)
        {
            let (stream, _) = listener.accept().await.unwrap();
            handle_conn(
                stream,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]",
            )
            .await;
        }
        // conn 1: POST /upload → {"uploadId":"test-upload-id"}
        let create_resp = b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 29\r\n\r\n{\"uploadId\":\"test-upload-id\"}";
        {
            let (stream, _) = listener.accept().await.unwrap();
            handle_conn(stream, create_resp).await;
        }
        // conn 2: PATCH chunk → 200
        {
            let (stream, _) = listener.accept().await.unwrap();
            handle_conn(stream, b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await;
        }
        // conn 3: POST complete — capture request, then respond 200
        {
            let (stream, _) = listener.accept().await.unwrap();
            let raw = handle_conn(stream, b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await;
            let _ = tx.send(raw);
        }
    });

    // ── write a small test file ────────────────────────────────────────
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let file_bytes: Vec<u8> = vec![0xDE, 0xAD, 0xBE];
    let file_path = tmp.path().join("chunk-test.bin");
    std::fs::write(&file_path, &file_bytes).unwrap();

    let expected_hash = hex::encode(Sha256::digest(&file_bytes));

    // ── run upload_file_chunked ────────────────────────────────────────
    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );

    // The caller routes to this path via `needs_chunking`, but
    // upload_file_chunked itself uses total_size (= file_size arg) to drive its
    // loop. Pass the actual byte count — 3 bytes fits in one request, so we get
    // exactly GET+POST+PATCH+POST.
    let file_size = file_bytes.len() as u64;
    let throughput = crate::seta::upload_policy::Throughput::new();
    client
        .upload_file_chunked(
            "her-blog",
            "test.bin",
            &file_path,
            file_size,
            "abc123def456abcd",
            &throughput,
            None,
        )
        .await
        .expect("upload_file_chunked must succeed against mock server");

    // ── verify header on the complete POST ────────────────────────────
    let complete_request = rx
        .await
        .expect("mock server must send complete request bytes");
    let request_str = String::from_utf8_lossy(&complete_request);

    // Find X-Moss-Content-Hash header value (case-insensitive name match).
    let hash_header_value = request_str.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("x-moss-content-hash:") {
            Some(line[line.find(':').unwrap() + 1..].trim().to_string())
        } else {
            None
        }
    });

    assert!(
        hash_header_value.is_some(),
        "complete POST must include X-Moss-Content-Hash header; request was:\n{}",
        request_str
    );
    assert_eq!(
        hash_header_value.unwrap(),
        expected_hash,
        "X-Moss-Content-Hash must equal sha256 of the uploaded file bytes"
    );
}

/// **Resuming must hash the prefix it did not send.**
///
/// The complete-POST's `X-Moss-Content-Hash` covers the whole reassembled file.
/// A resume at offset O that opens a fresh `Sha256` sends `sha256(file[O..])`;
/// the server answers HASH_MISMATCH, a 4xx is not transient so it is fatal, and
/// the failure path aborts the session — so the attempt transfers the entire
/// remainder and then destroys every byte the previous attempt had staged.
/// That is strictly worse than not resuming at all, which is why this test
/// exists rather than trusting the code path.
///
/// The mock hands back a session at offset 3 of an 8-byte file. Two assertions
/// carry it: the PATCH body must be exactly the last 5 bytes (proving the
/// resume happened), and the content hash must be over all 8 (proving the
/// prefix was rehashed).
#[tokio::test]
async fn a_resumed_upload_hashes_the_prefix_it_did_not_send() {
    use sha2::{Digest as _, Sha256};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const RESUME_AT: usize = 3;
    let file_bytes: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8];

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let requests: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));

    let seen = Arc::clone(&requests);
    let total = file_bytes.len();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                // Read headers, then exactly Content-Length body bytes, so the
                // captured PATCH body is complete rather than whatever the
                // first TCP read happened to carry.
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let Ok(n) = stream.read(&mut buf).await else { break };
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let Some(hdr_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let body_len = String::from_utf8_lossy(&raw[..hdr_end])
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .starts_with("content-length:")
                                .then(|| l.split(':').nth(1)?.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if raw.len() >= hdr_end + 4 + body_len {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&raw).to_string();
                let session = format!(
                    "[{{\"uploadId\":\"resumed-id\",\"filePath\":\"resume.bin\",\
                     \"size\":{},\"offset\":{}}}]",
                    total, RESUME_AT
                );
                let resp = if head.starts_with("GET") && head.contains("/uploads?") {
                    format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        session.len(),
                        session
                    )
                } else if head.starts_with("POST") && head.contains("/upload ") {
                    // A create-session POST here would mean the resume was
                    // ignored; answer it so the test fails on the assertion
                    // rather than on a hang.
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 26\r\n\r\n{\"uploadId\":\"fresh-id\"}\r\n\r\n".to_string()
                } else {
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_string()
                };
                seen.lock().unwrap().push(raw);
                stream.write_all(resp.as_bytes()).await.ok();
                stream.shutdown().await.ok();
            });
        }
    });

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let file_path = tmp.path().join("resume.bin");
    std::fs::write(&file_path, &file_bytes).unwrap();

    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let throughput = crate::seta::upload_policy::Throughput::new();
    client
        .upload_file_chunked(
            "her-blog",
            "resume.bin",
            &file_path,
            file_bytes.len() as u64,
            "abc123def456abcd",
            &throughput,
            None,
        )
        .await
        .expect("a resumed upload must succeed");

    let reqs = requests.lock().unwrap().clone();
    let text: Vec<String> = reqs
        .iter()
        .map(|r| String::from_utf8_lossy(r).to_string())
        .collect();

    assert!(
        !text.iter().any(|r| r.starts_with("POST") && r.contains("/upload ")),
        "a file the server already holds bytes for must adopt that session, not create a new one"
    );

    let patch = text
        .iter()
        .find(|r| r.starts_with("PATCH"))
        .expect("the remainder must still be sent");
    assert!(
        patch.contains(&format!("upload-offset: {}", RESUME_AT))
            || patch.contains(&format!("Upload-Offset: {}", RESUME_AT)),
        "the PATCH must continue from the server's offset; request was:\n{patch}"
    );
    let patch_raw = reqs
        .iter()
        .find(|r| r.starts_with(b"PATCH"))
        .expect("PATCH captured");
    let body_start = patch_raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("PATCH has headers")
        + 4;
    assert_eq!(
        &patch_raw[body_start..],
        &file_bytes[RESUME_AT..],
        "only the bytes the server is missing may be re-sent"
    );

    let complete = text
        .iter()
        .find(|r| r.contains("/complete"))
        .expect("the upload must be finalized");
    let sent_hash = complete
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .starts_with("x-moss-content-hash:")
                .then(|| l[l.find(':').unwrap() + 1..].trim().to_string())
        })
        .expect("complete POST must carry X-Moss-Content-Hash");
    assert_eq!(
        sent_hash,
        hex::encode(Sha256::digest(&file_bytes)),
        "the digest must cover the WHOLE file — a fresh hasher at a non-zero offset sends \
         sha256 of the tail, which the server rejects as HASH_MISMATCH and which then \
         aborts the very session the resume was for"
    );
}

/// A mock that answers every PATCH with `patch_status` and counts what it saw.
///
/// Returns `(patches, delete_seen, result)` for a 3-byte file — 3 bytes is far
/// below `MIN_ESCALATION_SIZE`, so there is no smaller request to try and the
/// escalation ladder is out of the picture entirely.
async fn upload_refused_with(patch_status: &'static str) -> (usize, bool, Result<(), SetaError>) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let patches = Arc::new(AtomicUsize::new(0));
    let deleted = Arc::new(AtomicBool::new(false));

    let seen = Arc::clone(&patches);
    let killed = Arc::clone(&deleted);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let seen = Arc::clone(&seen);
            let killed = Arc::clone(&killed);
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                // One read is enough to see the request line; the client sends
                // headers and a 3-byte body together.
                if let Ok(n) = stream.read(&mut buf).await {
                    raw.extend_from_slice(&buf[..n]);
                }
                let head = String::from_utf8_lossy(&raw);
                let refusal = format!("HTTP/1.1 {}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n", patch_status);
                let resp: &[u8] = if head.starts_with("GET") && head.contains("/uploads?") {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]"
                } else if head.starts_with("POST") && head.contains("/upload ") {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 29\r\n\r\n{\"uploadId\":\"test-upload-id\"}"
                } else if head.starts_with("PATCH") {
                    seen.fetch_add(1, Ordering::SeqCst);
                    refusal.as_bytes()
                } else {
                    if head.starts_with("DELETE") {
                        killed.store(true, Ordering::SeqCst);
                    }
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                };
                stream.write_all(resp).await.ok();
                stream.shutdown().await.ok();
            });
        }
    });

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let file_path = tmp.path().join("tail.bin");
    std::fs::write(&file_path, [0xDE, 0xAD, 0xBE]).unwrap();

    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let throughput = crate::seta::upload_policy::Throughput::new();
    let result = client
        .upload_file_chunked("her-blog", "tail.bin", &file_path, 3, "abc123def456abcd", &throughput, None)
        .await;
    (
        patches.load(Ordering::SeqCst),
        deleted.load(Ordering::SeqCst),
        result,
    )
}

/// A chunk already at or below the escalation floor must NOT be replayed once
/// its retries are spent.
///
/// The escalation decision has to key on the bytes actually SENT
/// (`chunk_size = min(remaining, request_size)`), not on `request_size`.
/// Keying it on `request_size` means the tail chunk of every chunked file —
/// and any whole file smaller than one chunk — sees `request_size` halve four
/// times (4 MB → 2 → 1 → 512 K → 256 K) while the bytes on the wire never
/// change: four more rounds of byte-identical PATCHes, each able to burn a full
/// request timeout. That is precisely the "replaying an identical request after
/// a deadline failure carries no new information" behaviour this module was
/// rewritten to end.
///
/// The mock answers 524 (the edge cut, and transient) to every PATCH, so the
/// count of PATCHes seen is exactly the number of attempts the client believed
/// were worth making: one `retry_transient` round and no escalation.
#[tokio::test]
async fn a_chunk_at_the_escalation_floor_is_not_replayed_after_its_retries() {
    let (patches, _, result) = upload_refused_with("524 ").await;
    assert!(result.is_err(), "every PATCH was refused; the upload must fail");
    assert_eq!(
        patches,
        crate::seta::client::RETRY_MAX_ATTEMPTS as usize,
        "the client must send exactly one round of retries and then stop — more \
         means it 'escalated' to a smaller request size while sending identical bytes"
    );
}

/// **A transient failure must leave the server's session alone.**
///
/// Aborting deletes the `.partial` the server is holding, so the next attempt
/// gets `[]` from `GET /uploads` and restarts the file at byte 0. On a slow
/// link that is unconditional: a 100 MB video at 50 KB/s stages ~80 MB, one
/// chunk runs out of retry budget, the abort deletes all 80 MB, and every
/// republish repeats it identically. The publish pays full price forever and
/// converges on nothing — invariant I2 ("every attempt strictly advances")
/// inverted on the ordinary failure path.
///
/// 524 is the exact failure that did it in production, and it is transient.
#[tokio::test]
async fn a_transient_failure_leaves_the_server_session_for_the_next_attempt() {
    let (_, deleted, result) = upload_refused_with("524 ").await;
    assert!(result.is_err());
    assert!(
        !deleted,
        "the client aborted the session after a transient failure, deleting the bytes the \
         server had already staged — the next attempt now has to start from zero"
    );
}

/// ...but a session that genuinely cannot be continued must still be cleaned
/// up, or a fatal 4xx strands a `.partial` on the server for its whole idle
/// window and every retry re-adopts the poisoned session.
#[tokio::test]
async fn a_fatal_failure_still_deletes_the_session() {
    let (_, deleted, result) = upload_refused_with("400 Bad Request").await;
    assert!(result.is_err());
    assert!(deleted, "a fatal failure must not strand the session on the server");
}

/// **The escalation ladder must be walked on a link that is actually slow.**
///
/// Its one previous test drove a mock that answered 524 *instantly*, which is
/// the only regime where the old gate could pass: `allows_another_attempt`
/// requires `futile_elapsed + UPLOAD_REQUEST_TIMEOUT <= UPLOAD_RETRY_BUDGET`,
/// so with real 150 s timeouts three failures spend 450 s of the 600 s window
/// and the *first* rung is the last. `escalate_down` fired only when failures
/// were free — never on the slow link it was written for. There was no test in
/// which escalation happened at all.
///
/// So the mock has to make each PATCH *expensive*: it reads the request, then
/// jumps the clock past the client's own request timeout, which is what a
/// saturated uplink looks like from here — every request times out, ~150 s
/// each, futile time accumulating. The assertion is that the client keeps
/// stepping DOWN; a client that gives up at the first rung sends one size and
/// stops.
///
/// # Why the clock is frozen *inside the mock* instead of for the whole test
///
/// The clock is only frozen for the moment between "the mock has the whole
/// request" and "the client's timeout has fired". Everywhere else this test
/// runs on the real clock, and that is load-bearing rather than tidiness.
///
/// A tokio test declared `start_paused = true` advances its clock by itself:
/// on any scheduler pass where nothing is immediately runnable, it jumps
/// straight to the next timer deadline. A task parked on a socket looks exactly
/// like "nothing runnable" for one such pass — so the clock could reach the
/// client's 150 s `UPLOAD_REQUEST_TIMEOUT` *before* the mock had read the
/// request off the socket. reqwest checks its overall timeout before it checks
/// the response, so the request then failed without the mock ever seeing it,
/// `sizes` stayed empty, and the first assertion below read `None`. That is the
/// macOS-only failure of PR #1000; measured here, 123 of 200 loopback requests
/// issued through reqwest under `start_paused` timed out spuriously.
///
/// So: **do not put `start_paused` back on this test, and do not widen the
/// freeze.** Freezing only once the request is recorded means the one thing the
/// jump can kill is the one request it is meant to kill; anywhere earlier and
/// it kills requests the test still needed to see.
///
/// The corollary is that the mock serves connections one at a time rather than
/// spawning a task each. Two handlers freezing the clock at once would panic
/// ("time is already frozen"), and the client under test issues exactly one
/// request at a time anyway.
#[tokio::test]
async fn a_slow_link_walks_the_escalation_ladder_down() {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let sizes: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));

    let seen = Arc::clone(&sizes);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            // One connection at a time — see the doc comment. `return` inside
            // this block leaves the block, not the accept loop.
            async {
                let mut raw = Vec::new();
                let mut buf = [0u8; 8192];
                // Headers only — the body is never drained, so nothing waits on
                // megabytes that are about to be abandoned anyway.
                while raw.windows(4).position(|w| w == b"\r\n\r\n").is_none() {
                    let Ok(n) = stream.read(&mut buf).await else { return };
                    if n == 0 {
                        return;
                    }
                    raw.extend_from_slice(&buf[..n]);
                }
                let head = String::from_utf8_lossy(&raw).to_string();
                let resp: &[u8] = if head.starts_with("GET") && head.contains("/uploads?") {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]"
                } else if head.starts_with("POST") && head.contains("/upload ") {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 29\r\n\r\n{\"uploadId\":\"test-upload-id\"}"
                } else if head.starts_with("PATCH") {
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .starts_with("content-length:")
                                .then(|| l.split(':').nth(1)?.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    seen.lock().unwrap().push(len);
                    // The link is too slow for this request: nothing comes back
                    // before the client's own UPLOAD_REQUEST_TIMEOUT fires. The
                    // request is already recorded, so freezing the clock here
                    // can only kill this request — which is the point.
                    tokio::time::pause();
                    tokio::time::advance(
                        crate::seta::upload_policy::UPLOAD_REQUEST_TIMEOUT
                            + std::time::Duration::from_secs(10),
                    )
                    .await;
                    tokio::time::resume();
                    return;
                } else {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                };
                stream.write_all(resp).await.ok();
                stream.shutdown().await.ok();
            }
            .await;
        }
    });

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = tempfile::TempDir::new_in(&base).unwrap();
    let file_path = tmp.path().join("video.bin");
    let size = crate::seta::upload_policy::INITIAL_REQUEST_SIZE;
    std::fs::write(&file_path, vec![7u8; size]).unwrap();

    let identity = crate::identity::Identity::generate().unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );
    let throughput = crate::seta::upload_policy::Throughput::new();
    let result = client
        .upload_file_chunked(
            "her-blog",
            "video.bin",
            &file_path,
            size as u64,
            "abc123def456abcd",
            &throughput,
            None,
        )
        .await;

    assert!(result.is_err(), "nothing ever succeeded; the upload must fail");
    let sizes = sizes.lock().unwrap().clone();
    // "No request arrived" and "escalation didn't happen" are different
    // failures; conflating them cost an hour of diagnosis on #992.
    assert!(
        !sizes.is_empty(),
        "the mock server recorded no request at all — the upload failed before \
         the first chunk went out (#992's flake shape), not per-chunk as this \
         test intends"
    );
    assert_eq!(sizes.first(), Some(&size), "the first request is the planned size");
    assert!(
        sizes.iter().any(|&s| s < size),
        "the client never escalated down: it sent {sizes:?} and gave up at the first rung, \
         which is what happens whenever failures are slow enough to matter"
    );
    assert!(
        sizes.iter().any(|&s| s <= size / 4),
        "escalation stopped after one rung; sent {sizes:?}"
    );
    assert!(
        sizes.windows(2).all(|w| w[1] <= w[0]),
        "request sizes must be monotonically decreasing; sent {sizes:?}"
    );
}
