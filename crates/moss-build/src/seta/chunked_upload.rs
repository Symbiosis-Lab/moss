//! The chunked (resumable) upload protocol.
//!
//! Split out of `sites.rs` 2026-08-03. It is a self-contained five-endpoint
//! protocol — create session, PATCH chunks, HEAD for the authoritative offset,
//! complete, abort — with its own failure handling, and it has nothing to do
//! with the site registration / metadata / analytics endpoints that make up the
//! rest of `sites.rs`. Keeping them together pushed that file past its
//! prod-line budget; the split is the boundary the budget was pointing at.
//!
//! Sizing (chunk size, concurrency, request timeout, retry escalation) lives in
//! `upload_policy`.

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::client::{MossSetaClient, SetaError};
use super::failure_class::{classify, FailureClass};
use super::sites::UploadSession;
use super::upload_policy::{Throughput, UPLOAD_REQUEST_TIMEOUT};

impl MossSetaClient {
    /// Upload a large file using the resumable chunked upload protocol.
    ///
    /// Splits the file into chunks sized against the link the deploy has
    /// measured, so each PATCH completes well inside Cloudflare's 125s proxy
    /// read timeout however slow the uplink is. Sequence:
    ///   1. POST `/:id/upload` — create session, receive `uploadId`
    ///   2. PATCH `/:id/upload/:uploadId` — one request per chunk
    ///   3. POST `/:id/upload/:uploadId/complete` — finalize, move to site path
    ///
    /// `file_size` is passed in by the caller (who already has it from the
    /// threshold check) to avoid a redundant `metadata` syscall here.
    ///
    /// On failure the session is aborted (best-effort DELETE) **only when it is
    /// genuinely unusable** — see [`abort_if_unusable`](Self::abort_if_unusable).
    /// A transient failure or an exhausted retry budget leaves the server's
    /// `.partial` where it is so the next attempt resumes from it; the server's
    /// idle sweep handles sessions nobody comes back for.
    ///
    /// Called by `deploy::upload` for files that do not fit one request at the
    /// currently measured link speed (`upload_policy::needs_chunking`).
    ///
    /// `generation_id` is sent as `X-Moss-Generation` on the **create-session**
    /// POST only. The server stores the generation in the upload session at
    /// create time, so per-chunk PATCH and complete POST do not repeat it.
    ///
    /// Before creating a session it asks the server whether it already holds
    /// one for this file and generation, and resumes it — invariant I2, "every
    /// attempt strictly advances". Costs one small GET per chunked file, which
    /// is cheap against a file large enough to be chunked at all.
    pub async fn upload_file_chunked(
        &self,
        site_id: &str,
        file_path: &str,
        local_path: &std::path::Path,
        file_size: u64,
        generation_id: &str,
        // The deploy's shared link estimate: sizes the first PATCH, and is
        // updated from every PATCH that lands.
        throughput: &Throughput,
        // Reports `bytes_just_uploaded` after each successful chunk PATCH, so the
        // deploy hairline moves smoothly through a large file instead of jumping
        // only at file completion. `None` from tests / contexts without progress
        // tracking. (Design 2026-06-11 §1c.)
        on_chunk: Option<&(dyn Fn(u64) + Send + Sync)>,
    ) -> Result<(), SetaError> {
        use tokio::io::AsyncReadExt;

        let total_size = usize::try_from(file_size).map_err(|_| {
            SetaError::Io(format!(
                "file too large to upload: {} bytes exceeds platform address space",
                file_size
            ))
        })?;

        log::debug!(
            target: "seta",
            "chunked upload start: {} ({} bytes) → site={}",
            file_path, total_size, site_id
        );
        throughput.note_chunked_file();
        let file_started = std::time::Instant::now();
        // Per-file request accounting for the completion INFO line.
        // `requests - requests_ok` is every attempt that bought nothing:
        // transient retries, escalation re-sends, resynced offsets alike.
        let file_requests = std::sync::atomic::AtomicU64::new(0);
        let file_requests_ok = std::sync::atomic::AtomicU64::new(0);

        // 1. Resume the server's session for this file if it has one, else
        //    create a fresh session. The returned hasher is already primed with
        //    `file[0..offset]` — see `resume_or_create_session`.
        let (upload_id, start_offset, mut hasher) = self
            .resume_or_create_session(site_id, file_path, local_path, total_size, generation_id)
            .await?;

        // 2. Upload chunks sequentially.
        let encoded_id = urlencoding::encode(&upload_id).into_owned();
        // allow:raw_read built output being uploaded — regenerable, dataless is absent
        let mut file = tokio::fs::File::open(local_path).await.map_err(|e| {
            SetaError::Io(format!("open {}: {}", local_path.display(), e))
        })?;
        let mut offset: usize = start_offset;
        if offset > 0 {
            if let Err(e) = seek_to(&mut file, offset).await {
                self.abort_if_unusable(site_id, &encoded_id, &e).await;
                return Err(e);
            }
            if let Some(cb) = on_chunk {
                // Credit the resumed prefix, or the deploy's byte counter would
                // under-report by everything a previous attempt already sent.
                cb(offset as u64);
            }
        }

        // Hard ceiling on this file's remaining chunks, lowered only by
        // `escalate_down` after a request was actually cut. The predictive
        // half of sizing is the per-chunk plan below; this is the reactive
        // half's memory — evidence that a size did not fit this link must not
        // be overruled by a rising estimate mid-file. Replaying an identical
        // request after a 524 is what spent that vault's whole 600s budget on
        // three failures that each took exactly as long as the first.
        let mut escalation_cap: usize = usize::MAX;

        // Bounded resyncs against the server's authoritative offset. Bounded
        // because a resync that does not converge would loop forever on a
        // server whose offset never advances.
        let mut resyncs: u32 = 0;
        const MAX_RESYNCS: u32 = 3;

        // ONE budget for the whole file, shared by every chunk: a 10-chunk file
        // must not get 10 independent 4-attempt allowances, or its worst case
        // scales with size and outruns the deploy budget.
        let budget = crate::seta::client::RetryBudget::new();

        // `hasher` is a running sha256 over the full file bytes in chunk order,
        // so the server can verify the reassembled file matches what the client
        // sent. It arrives from `resume_or_create_session` already covering
        // `file[0..offset]`.
        //
        // Updated only AFTER a chunk is confirmed appended. Hashing before the
        // PATCH desynchronises the two hashers the moment a chunk is re-sent at
        // a different size or the offset resyncs: the client would have hashed
        // bytes the server never appended, or hashed them twice, and the
        // failure would surface as HASH_MISMATCH after the ENTIRE file had
        // transferred — the worst possible shape.

        while offset < total_size {
            // Re-plan per chunk from the live estimate AND the live occupancy.
            // A link that collapses partway through a large file gets smaller
            // chunks predictively instead of rediscovering the slowdown by
            // burning a 150 s timeout per chunk; a file that briefly shared
            // the window gets its full-share size back when the siblings
            // finish, rather than staying pinned to the divided size for its
            // remaining tens of megabytes. Only `escalation_cap` is monotone:
            // it holds evidence (a request the edge actually cut), which a
            // rising estimate must not overrule mid-file.
            let request_size = throughput.plan_request_size().min(escalation_cap);
            let chunk_size = (total_size - offset).min(request_size);
            let mut chunk = vec![0u8; chunk_size];
            if let Err(e) = file.read_exact(&mut chunk).await {
                let e = SetaError::Io(format!("read chunk at offset {}: {}", offset, e));
                self.abort_if_unusable(site_id, &encoded_id, &e).await;
                return Err(e);
            }

            // Retry the sign+PATCH unit; abort-session stays OUTSIDE it, so a
            // recoverable blip re-sends the chunk instead of killing the session.
            let patch_path = format!("/api/sites/{}/upload/{}", site_id, encoded_id);
            let chunk = bytes::Bytes::from(chunk);
            let (this, patch_path, chunk_ref, tp) =
                (self, patch_path.as_str(), &chunk, throughput);
            let (file_reqs, file_oks) = (&file_requests, &file_requests_ok);
            let sent = crate::seta::client::retry_transient(file_path, &budget, || async move {
                let (patch_url, patch_auth) = this.sign_and_build(patch_path)?;
                tp.note_request();
                file_reqs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let started = std::time::Instant::now();
                let resp = this
                    .client
                    .patch(&patch_url)
                    .timeout(UPLOAD_REQUEST_TIMEOUT)
                    .header("Authorization", patch_auth)
                    .header("Content-Type", "application/octet-stream")
                    .header("Upload-Offset", offset.to_string())
                    .body(chunk_ref.clone())
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    return Err(SetaError::from_failed_response(resp).await);
                }
                // Measured per ATTEMPT, not per chunk: a chunk that failed once
                // and succeeded on retry took two requests, and only the one
                // that actually moved its bytes end-to-end is a bandwidth
                // sample. Timed around the send so the next chunk of THIS file
                // is re-planned against what this link just did (see the
                // downward re-plan at the top of the loop).
                tp.observe(chunk_ref.len() as u64, started.elapsed());
                tp.note_request_ok();
                file_oks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            })
            .await;
            if let Err(e) = sent {
                // The server's offset is authoritative. A 409 means a previous
                // PATCH landed differently than we believe — most often a body
                // truncated by the edge that still appended some bytes. Before
                // 2026-08-03 this killed the session and restarted the whole
                // file; now we ask where the server actually is and continue
                // from there. seta has served this endpoint since it was
                // written; moss just never called it.
                if matches!(&e, SetaError::Api { status, .. } if *status == 409)
                    && resyncs < MAX_RESYNCS
                    && budget.allows_another_attempt(std::time::Duration::ZERO)
                {
                    resyncs += 1;
                    match self.chunked_upload_offset(site_id, &encoded_id).await {
                        Ok(server_offset) if server_offset <= total_size => {
                            log::warn!(
                                target: "seta",
                                "chunked upload {}: offset mismatch at {}, resyncing to server offset {} (resync {}/{})",
                                file_path, offset, server_offset, resyncs, MAX_RESYNCS
                            );
                            // Rebuild both the file cursor and the hash state
                            // from the server's offset. The hasher has no
                            // rewind, so re-read the prefix — one local read of
                            // a file we are about to spend far longer sending.
                            match rehash_prefix(local_path, server_offset).await {
                                Ok(h) => {
                                    hasher = h;
                                    offset = server_offset;
                                    if let Err(io) = seek_to(&mut file, server_offset).await {
                                        self.abort_if_unusable(site_id, &encoded_id, &io).await;
                                        return Err(io);
                                    }
                                    continue;
                                }
                                Err(io) => {
                                    self.abort_if_unusable(site_id, &encoded_id, &io).await;
                                    return Err(io);
                                }
                            }
                        }
                        _ => {
                            self.abort_if_unusable(site_id, &encoded_id, &e).await;
                            return Err(e);
                        }
                    }
                }

                // A timeout or an edge cut means this request was too big for
                // the bandwidth available. Retry it smaller rather than
                // identically; the shared budget still bounds the total.
                let too_slow = matches!(&e, SetaError::Api { status, .. } if *status == 524)
                    || matches!(&e, SetaError::Http(h) if h.is_timeout());
                // Escalate from what was actually SENT, not from `request_size`.
                // The tail chunk of any file — and `chunk_size` is
                // `min(remaining, request_size)` — is smaller than
                // `request_size`, so halving `request_size` would leave
                // `chunk_size` unchanged and re-send a byte-identical PATCH.
                // That is the exact behaviour this escalation exists to end.
                let smaller = crate::seta::upload_policy::escalate_down(chunk_size);
                // Tell the budget the next request is DIFFERENT before asking
                // it. Its futile-time stopwatch measures repetition that
                // achieves nothing, and stepping down the ladder is the one
                // retry that is not repetition — without this the gate below is
                // unreachable in exactly the case escalation exists for, since
                // three 150 s timeouts have already spent 450 s of a 600 s
                // window and no attempt can be *admitted* to finish inside what
                // is left. The ladder then only ever fired when failures were
                // instant, i.e. never on a slow link. `MAX_FAILED_ATTEMPTS_PER_FILE`
                // (never reset) and the ladder's own floor still terminate it.
                let escalating = too_slow && smaller < chunk_size;
                if escalating {
                    budget.note_escalation();
                }
                if escalating && budget.allows_another_attempt(std::time::Duration::ZERO) {
                    log::warn!(
                        target: "seta",
                        "chunked upload {}: {} byte request did not fit the link, retrying at {} bytes",
                        file_path, chunk_size, smaller
                    );
                    escalation_cap = smaller;
                    // `offset` is unchanged and the hasher was never updated for
                    // this chunk, so re-reading from `offset` is exact. Rewind
                    // the file cursor, which read_exact advanced.
                    if let Err(io) = seek_to(&mut file, offset).await {
                        self.abort_if_unusable(site_id, &encoded_id, &io).await;
                        return Err(io);
                    }
                    continue;
                }

                // The one that matters: `e` here is whatever `retry_transient`
                // gave up on, including "out of retry budget" on a link that is
                // simply slow. Aborting on that deletes the megabytes this
                // attempt just staged and guarantees the next one starts at
                // zero — see `abort_if_unusable`.
                self.abort_if_unusable(site_id, &encoded_id, &e).await;
                return Err(e);
            }

            // Hash only what the server confirmed it appended, and only once.
            hasher.update(&chunk);

            // Report this chunk's bytes (after the PATCH succeeded) so the deploy
            // hairline advances mid-file, not only at file completion.
            if let Some(cb) = on_chunk {
                cb(chunk_size as u64);
            }

            offset += chunk_size;
            log::debug!(
                target: "seta",
                "chunked upload {}: {}/{} bytes",
                file_path, offset, total_size
            );
        }

        // 3. Finalize.
        // Compute the content hash over all chunks (streaming sha256 over the
        // complete file bytes in upload order). The server reassembles chunks
        // in offset order and can independently compute the same sha256 —
        // identical sequential input guarantees identical digest.
        let content_hash = hex::encode(hasher.finalize());

        let complete_path = format!(
            "/api/sites/{}/upload/{}/complete",
            site_id, encoded_id
        );
        let (complete_url, complete_auth) = match self.sign_and_build(&complete_path) {
            Ok(v) => v,
            Err(e) => {
                self.abort_if_unusable(site_id, &encoded_id, &e).await;
                return Err(e);
            }
        };
        let resp = self
            .client
            .post(&complete_url)
            .timeout(UPLOAD_REQUEST_TIMEOUT)
            .header("Authorization", complete_auth)
            .header("Content-Length", "0")
            .header("X-Moss-Content-Hash", &content_hash)
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => {
                let err = SetaError::from_failed_response(r).await;
                self.abort_if_unusable(site_id, &encoded_id, &err).await;
                return Err(err);
            }
            Err(e) => {
                let e = SetaError::Http(e);
                self.abort_if_unusable(site_id, &encoded_id, &e).await;
                return Err(e);
            }
        }

        // The per-file half of the completion summary: everything a support log has
        // to say about this file's transfer, greppable as `chunked upload
        // complete`. Rate is over the bytes this session actually sent — a
        // resumed prefix is progress, not throughput.
        let sent_bytes = (total_size - start_offset) as u64;
        let secs = file_started.elapsed().as_secs_f64();
        let requests = file_requests.load(std::sync::atomic::Ordering::Relaxed);
        let retried =
            requests.saturating_sub(file_requests_ok.load(std::sync::atomic::Ordering::Relaxed));
        log::info!(
            target: "seta",
            "chunked upload complete: {} — {} bytes in {:.0}s = {} B/s, {} requests, {} retried; estimate {} B/s",
            file_path,
            sent_bytes,
            secs,
            (sent_bytes as f64 / secs.max(0.001)) as u64,
            requests,
            retried,
            throughput.bytes_per_sec(),
        );
        Ok(())
    }

    /// Adopt the server's existing session for this file, or create a new one.
    ///
    /// Returns `(upload_id, offset, hasher)` where **`hasher` already covers
    /// `file[0..offset]`**. That is the whole point of doing this in one
    /// function rather than two.
    ///
    /// # Why the prefix rehash is a precondition, not an optimisation
    ///
    /// The complete-POST sends `X-Moss-Content-Hash` and the server verifies it
    /// against the *whole reassembled file*. Resuming at offset `O` with a
    /// fresh `Sha256` therefore sends `sha256(file[O..])`, the server answers
    /// HASH_MISMATCH, that 4xx is not transient so it is fatal, and the failure
    /// path `abort_chunked_upload`s the session — destroying the very progress
    /// the resume was for. Resume without the rehash is **strictly worse than
    /// no resume**: it transfers the whole remainder and then throws away
    /// everything. So the rehash happens here, before the caller can enter the
    /// chunk loop at a non-zero offset.
    ///
    /// If the prefix cannot be re-read (the local file changed or shrank since
    /// the session was created), the session is aborted and a fresh one is
    /// created. Falling back must always be possible: a resume that could fail
    /// permanently would make a state unreachable, and the server keeps
    /// sessions for 24 h — a poisoned one would be re-adopted on every retry.
    async fn resume_or_create_session(
        &self,
        site_id: &str,
        file_path: &str,
        local_path: &std::path::Path,
        total_size: usize,
        generation_id: &str,
    ) -> Result<(String, usize, Sha256), SetaError> {
        if let Some(session) = self
            .resumable_session(site_id, file_path, total_size, generation_id)
            .await
        {
            let offset = session.offset as usize;
            if offset == 0 {
                return Ok((session.upload_id, 0, Sha256::new()));
            }
            match rehash_prefix(local_path, offset).await {
                Ok(hasher) => {
                    log::info!(
                        target: "seta",
                        "chunked upload {}: resuming server session at {}/{} bytes",
                        file_path, offset, total_size
                    );
                    return Ok((session.upload_id, offset, hasher));
                }
                Err(e) => {
                    log::warn!(
                        target: "seta",
                        "chunked upload {}: cannot rehash the resumed prefix ({}), \
                         discarding the server session and starting over",
                        file_path, e
                    );
                    let encoded = urlencoding::encode(&session.upload_id).into_owned();
                    self.abort_chunked_upload(site_id, &encoded).await;
                }
            }
        }

        let upload_id = self
            .create_upload_session(site_id, file_path, total_size, generation_id)
            .await?;
        Ok((upload_id, 0, Sha256::new()))
    }

    /// The server's session for exactly this file, if it has one.
    ///
    /// Matched on path AND declared size: two builds can put different bytes at
    /// the same path, and adopting the wrong one would splice two files
    /// together. A failure to list is not a failure to upload — resume may only
    /// ever remove work — so any error degrades to "no session".
    ///
    /// When several sessions match, take the **furthest along**. Duplicates for
    /// one (generation, path) are reachable without anything being wrong: a
    /// transient failure of this very LIST call degrades to "no session", the
    /// attempt creates a second one, and now the server holds both. Adopting
    /// whichever the server happened to list first would silently discard the
    /// other one's progress, which is the same I2 inversion as aborting.
    async fn resumable_session(
        &self,
        site_id: &str,
        file_path: &str,
        total_size: usize,
        generation_id: &str,
    ) -> Option<UploadSession> {
        match self.list_upload_sessions(site_id, generation_id).await {
            Ok(sessions) => sessions
                .into_iter()
                .filter(|s| {
                    s.file_path == file_path
                        && s.size == total_size as u64
                        && s.offset <= total_size as u64
                })
                .max_by_key(|s| s.offset),
            Err(e) => {
                log::debug!(
                    target: "seta",
                    "chunked upload {}: could not list server sessions ({}), starting at zero",
                    file_path, e
                );
                None
            }
        }
    }

    /// POST `/api/sites/:id/upload` — create a session, return its `uploadId`.
    ///
    /// X-Moss-Generation is set here (create-session) only — the server stores
    /// the generation_id in the upload session at create time, so per-chunk
    /// PATCH and complete POST do not repeat it.
    async fn create_upload_session(
        &self,
        site_id: &str,
        file_path: &str,
        total_size: usize,
        generation_id: &str,
    ) -> Result<String, SetaError> {
        let create_path = format!("/api/sites/{}/upload", site_id);
        let (create_url, create_auth) = self.sign_and_build(&create_path)?;
        let response = self
            .client
            .post(&create_url)
            .timeout(UPLOAD_REQUEST_TIMEOUT)
            .header("Authorization", create_auth)
            .header("X-Moss-Generation", generation_id)
            .json(&serde_json::json!({ "filePath": file_path, "size": total_size }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }
        #[derive(Deserialize)]
        struct CreateResp {
            #[serde(rename = "uploadId")]
            upload_id: String,
        }
        Ok(response
            .json::<CreateResp>()
            .await
            .map_err(|e| SetaError::Parse(e.to_string()))?
            .upload_id)
    }

    /// Ask the server how many bytes of this session it actually holds.
    ///
    /// `HEAD /api/sites/:id/upload/:uploadId` -> `Upload-Offset`. seta has
    /// served this since the chunked protocol was written and its own comment
    /// claims "the moss client uses HEAD" — which was not true until 2026-08-03.
    /// This is dead capability being switched on, not new protocol.
    ///
    /// Signed per call: `retry_transient`'s contract is that every attempt
    /// re-signs, because seta rejects a clock skew over 300s.
    async fn chunked_upload_offset(
        &self,
        site_id: &str,
        encoded_upload_id: &str,
    ) -> Result<usize, SetaError> {
        let path = format!("/api/sites/{}/upload/{}", site_id, encoded_upload_id);
        let (url, auth) = self.sign_and_build(&path)?;
        let resp = self
            .client
            .head(&url)
            .timeout(UPLOAD_REQUEST_TIMEOUT)
            .header("Authorization", auth)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SetaError::from_failed_response(resp).await);
        }
        resp.headers()
            .get("Upload-Offset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or_else(|| SetaError::Parse("Upload-Offset header missing or unparseable".into()))
    }

    /// Abort the session, but only if this failure makes it unusable.
    ///
    /// # Why "on failure, abort" inverts the invariant it was written under
    ///
    /// Aborting deletes the server's `.partial` — i.e. it deletes exactly the
    /// progress resume exists to keep. That was harmless while a failed upload
    /// always restarted from zero anyway; it is the bug once the next attempt
    /// is supposed to resume.
    ///
    /// Concretely: a 100 MB video at 50 KB/s uploads for ~1600 s, gets 80 MB in,
    /// and then one chunk exhausts the retry budget. Aborting there throws away
    /// the 80 MB the server is holding, so the republish `GET /uploads` returns
    /// `[]` and starts at byte 0 — and does the same thing next time, and the
    /// time after. Every attempt pays full price and ends at zero, which is I2
    /// ("every attempt strictly advances") exactly inverted.
    ///
    /// So the abort is conditioned on the failure class, not on the fact of
    /// failure. [`FailureClass::Fatal`] is "this session cannot be continued" —
    /// a 4xx the server will keep answering, or a local defect (an unreadable
    /// file, a prefix that will not rehash, an identity that cannot sign)
    /// which no number of retries fixes and which would leave a session
    /// stranded for its full idle window. Everything else — a timeout, a 5xx,
    /// backpressure, an exhausted budget on a link that is merely slow — is a
    /// reason to *come back*, and the session is what makes coming back cheap.
    async fn abort_if_unusable(&self, site_id: &str, encoded_upload_id: &str, error: &SetaError) {
        if classify(error) == FailureClass::Fatal {
            self.abort_chunked_upload(site_id, encoded_upload_id).await;
        }
    }

    /// Abort a chunked upload session (best-effort DELETE).
    ///
    /// Uses a short timeout — this is a bodiless DELETE and we don't want a
    /// stalled network to extend an already-failed upload by minutes. Errors
    /// are silently swallowed; the server's hourly stale-session sweep will
    /// clean up any sessions we fail to abort explicitly.
    async fn abort_chunked_upload(&self, site_id: &str, encoded_upload_id: &str) {
        const ABORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
        let Ok((url, auth)) = self.sign_and_build(&format!(
            "/api/sites/{}/upload/{}",
            site_id, encoded_upload_id
        )) else {
            return;
        };
        let _ = self
            .client
            .delete(&url)
            .timeout(ABORT_TIMEOUT)
            .header("Authorization", auth)
            .send()
            .await;
    }
}

/// Rewind a file cursor to an absolute byte offset.
///
/// `read_exact` advances the cursor even on the paths where we then decide not
/// to send those bytes (a smaller retry, an offset resync). Without the rewind
/// the next read would silently skip content and the upload would complete with
/// a hole in it — caught only by the server's hash check, after the whole file
/// had transferred.
async fn seek_to(file: &mut tokio::fs::File, offset: usize) -> Result<(), SetaError> {
    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(offset as u64))
        .await
        .map(|_| ())
        .map_err(|e| SetaError::Io(format!("seek to {}: {}", offset, e)))
}

/// Sha256 state as it would be after streaming the first `len` bytes of `path`.
///
/// Needed because Sha256 cannot rewind, and an offset resync moves the client
/// backwards. Reads in 64 KB pieces so a resync on a large video does not
/// buffer it.
async fn rehash_prefix(
    path: &std::path::Path,
    len: usize,
) -> Result<Sha256, SetaError> {
    use tokio::io::AsyncReadExt;
    // allow:raw_read built output being uploaded — regenerable, dataless is absent
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| SetaError::Io(format!("reopen {}: {}", path.display(), e)))?;
    let mut hasher = Sha256::new();
    let mut remaining = len;
    let mut buf = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let want = remaining.min(buf.len());
        let n = file
            .read(&mut buf[..want])
            .await
            .map_err(|e| SetaError::Io(format!("re-read {}: {}", path.display(), e)))?;
        if n == 0 {
            return Err(SetaError::Io(format!(
                "re-read {}: file is shorter than the server's offset {}",
                path.display(),
                len
            )));
        }
        hasher.update(&buf[..n]);
        remaining -= n;
        // A resume can re-read tens of megabytes before the first PATCH of this
        // file, crediting no bytes; the stall watchdog must see it as progress.
        crate::infra::liveness::bump();
    }
    Ok(hasher)
}

#[cfg(test)]
#[path = "chunked_upload_tests.rs"]
mod tests;
