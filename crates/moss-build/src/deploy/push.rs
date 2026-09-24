//! One moss-hosted publish, from a sealed manifest to a live site.
//!
//! The whole of what `push_site` does once its inputs are resolved: the freeze
//! latch, the manifest sync, the upload window, the mass-removal gate, the
//! commit, the event sync, and the record of what went live. Crossed here
//! 2026-09-09 (track C4e) — the body had already stopped naming a `tauri` type
//! at C4d, so this move is what makes that fact reachable rather than merely
//! true.
//!
//! What did NOT cross is everything *before* the body, and it is app-shaped for
//! one reason: a long-lived process with a file watcher has in-flight build work
//! to drain and a sealed manifest sitting in session state. A terminal publish
//! has neither — it builds once, synchronously, and holds the manifest it just
//! sealed. So `src-tauri/src/deploy.rs` keeps the resolution, and each binary
//! reaches this function with its own answers.

use std::path::Path;

use crate::build::manifest::SealedManifest;
use crate::deploy::progress;
use crate::deploy::{landed, mass_remove, upload};
use crate::deploy::{view_site_url, PushResult, MOSSPUB_VPS_IP};
use crate::identity::Identity;
use crate::moss_paths::MossPaths;

/// Everything a publish needs besides its manifest. The manifest stays a
/// separate parameter on purpose: `&SealedManifest` IS the deploy contract, and
/// carrying it visibly is what makes it impossible to deploy before the
/// artifacts are registered. `sink` is threaded rather than rebuilt from an
/// `AppHandle` at each site, so one publish has exactly one reporter.
///
/// **No `tauri` type appears here.** It held an `AppHandle` and a
/// `State<AppState>` until track C4d; every read of either turned out to be
/// one of four questions, and those are now `ports`. What is left is a path,
/// a key, a name, a reporter and a set of answers — the publish body is
/// already binary-agnostic, and this struct is where that is checkable.
pub struct PushContext<'a> {
    pub folder_path: &'a Path,
    pub identity: &'a Identity,
    pub site_id: &'a str,
    pub sink: &'a std::sync::Arc<dyn progress::DeploySink>,
    pub ports: &'a dyn crate::build::ports::deploy::DeployPorts,
    /// Serializes event sync for this site. A value, not a port call: the
    /// publish already knows its `site_id`, and what comes back is a plain
    /// `Arc` either way. The app hands over the one from its per-site map so a
    /// second publish of the same site waits; a terminal publish is alone in
    /// its process and passes a fresh one.
    pub events_lock: &'a std::sync::Arc<tokio::sync::Mutex<()>>,
}

/// Map a push outcome to a stable, greppable status label for the
/// `=== DEPLOY END … status=… ===` boundary marker written to the log.
///
/// Not [`PushResult::landed`] with two names for `false`: this labels the
/// `Err` arm too, and the three labels are what a log reader greps for —
/// `needs_setup` and `failed` are the same answer to "did bytes land" and
/// completely different things to find in a log. Its exhaustive match is what
/// makes a new variant pick a label instead of inheriting one.
fn deploy_status_label(result: &Result<PushResult, String>) -> &'static str {
    match result {
        Ok(PushResult::Success { .. }) => "success",
        Ok(PushResult::NeedsSetup) => "needs_setup",
        Err(_) => "failed",
    }
}

/// Type-gated entry point for deploy work, and the one all internal callers
/// (CLI, tests, plugins, future parallel deploy paths) should use. The
/// `&SealedManifest` parameter is the deploy contract: the signature makes it
/// impossible to call deploy before all artifacts are registered (see
/// `docs/reference/build-pipeline.md`). The `push_site` Tauri command
/// resolves the manifest from `AppState` and forwards here.
///
/// Brackets the deploy with `=== DEPLOY START/END … status=… ===` boundary
/// markers so the (now much smaller) failure span is grep-locatable by
/// `DEPLOY END … status=failed`; `deploy_id` correlates the two lines. A
/// `NeedsSetup` short-circuits earlier in `push_site_command_body`, so reaching
/// here is always a genuine deploy attempt.
pub async fn push_site_inner(sealed: &SealedManifest, cx: &PushContext<'_>) -> Result<PushResult, String> {
    let full = uuid::Uuid::new_v4().simple().to_string();
    let deploy_id = full.get(..8).unwrap_or(&full);
    let site_id = cx.site_id;
    log::info!("=== DEPLOY START {deploy_id} site={site_id} ===");
    // Bound the deploy body by STALLING, not by total duration: the 900s
    // DEPLOY_TIMEOUT this replaces killed publishes that were transferring
    // perfectly well (57.66 MB over 50 KB/s needs ~1142s — I1, see
    // deploy/activity.rs). Cancelling is unchanged: the future is dropped,
    // in-flight requests cancel, locks release, the UI gets a retryable error.
    let result = crate::infra::liveness::bounded_by_stall(
        push_site_inner_impl(sealed, cx),
        crate::infra::liveness::STALL_TIMEOUT,
        &format!("deploy {deploy_id}"),
    )
    .await;
    log::info!(
        "=== DEPLOY END {deploy_id} status={} ===",
        deploy_status_label(&result)
    );
    result
}

/// Credit bytes the server confirmed — the upload phase's only proof that the
/// publish is moving, and so the only place it bumps the stall clock.
fn credit_upload_bytes(st: &progress::UploadProgressState, n: u64) {
    st.bytes_uploaded.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    crate::infra::liveness::bump();
}

/// Implementation of the deploy work. **Do not call directly** — call
/// [`push_site_inner`], which brackets this with the `=== DEPLOY START/END ===`
/// boundary markers; calling `_impl` directly bypasses them. The
/// `&SealedManifest` contract (deploy cannot run before artifacts are
/// registered) is documented on the wrapper and in
/// `docs/reference/build-pipeline.md`.
async fn push_site_inner_impl(
    sealed: &SealedManifest,
    cx: &PushContext<'_>,
) -> Result<PushResult, String> {
    let PushContext { folder_path, identity, site_id, sink, ports, events_lock } = *cx;
    let folder_path_str = folder_path.to_string_lossy().to_string();

    // Refuse before touching the network, through the door guard every
    // config-reading door shares (`site_config::ensure_config_current`).
    crate::build::site_config::ensure_config_current(&folder_path_str)?;

    // `AppState::active_environment()` is `resolve_environment(project_path)`
    // and nothing else, and this function has the folder in hand. Reading it
    // straight off the folder deletes four `AppState` reads from a body that
    // C4d is trying to reach without one, and cannot disagree with the app's
    // answer: it is the same function over the same path.
    let environment = crate::build::site_config::resolve_environment(&folder_path_str);

    // Pin this generation so a concurrent build's GC storm cannot evict the
    // directory we are uploading from. The guard is RAII — it releases the pin
    // on every exit path: normal return, `?`-early-return, panic, AND the
    // stall-watchdog future-drop in `push_site_inner` (the losing `select!`
    // branch is dropped, which runs all Drop impls it captured).
    let _pin = crate::build::ports::deploy::pin_generation(ports, sealed.generation_id());

    // The wire manifest is exactly the sealed manifest's files() — redirect stubs
    // are now emitted during generate_blocking_content (Gap #3 fix) so the seal
    // covers them and the generation-id is stable. No post-seal mutation of the
    // generation directory.
    let mut manifest = sealed.files().clone();

    let site_id = site_id.to_string();

    // 6. Emit: syncing
    sink.stage(progress::DeployStage::Syncing, 0, 0, "Comparing with server...");

    // 7. Create seta client. Client-side subscription pre-flight removed
    // 2026-04-22: the server-side per-site subscription rewrite (moss-seta#106)
    // moved access enforcement into the sync/commit endpoints themselves. This
    // pre-flight was reading a `whoami.subscription` field that no longer
    // exists, causing false `subscription_expired` errors on every deploy.
    // See moss#538 for the proper rebuild against `/api/subscriptions/:siteId`.
    let client =
        crate::seta::client::MossSetaClient::for_environment(identity, &environment);

    // Short-circuit: if the server is already live on the generation we're
    // about to deploy, skip the whole sync/upload/commit cycle. Any Err or
    // Ok(None) from get_live_generation (404 = old server, network error, site
    // never deployed) falls through to the normal path for back-compat.
    if let Ok(Some(live_gen)) = client.get_live_generation(&site_id).await {
        if live_gen == sealed.generation_id() {
            log::info!(
                "deploy: generation {} already live on server, skipping deploy",
                live_gen
            );
            // No Complete emit: returning Ok reaches `with_publish_guard`, which
            // emits it. The message was never read — both listeners ignore it on
            // a terminal stage.
            let url = view_site_url(&folder_path_str, &site_id, environment);
            return Ok(PushResult::Success {
                url,
                files_uploaded: 0,
                files_removed: 0,
            });
        }
    }

    let diff = client
        .sync_manifest(&site_id, &manifest, sealed.generation_id())
        .await
        .map_err(|e| e.to_string())?;

    if diff.need.is_empty() && diff.remove.is_empty() {
        let url = view_site_url(&folder_path_str, &site_id, environment);
        return Ok(PushResult::Success {
            url,
            files_uploaded: 0,
            files_removed: 0,
        });
    }

    // The server's diff used to be pushed here as a `PublishChangeSet`, so the
    // button could say "54 files uploading" on a first publish (no local record
    // ⇒ the resting change set carries no counts). It is gone: that answers
    // "what is this publish transferring", not "what would publishing change",
    // and putting two questions on one channel is what forced the frontend to
    // guess which one it was holding. The same two numbers now ride
    // `DeployProgress` — from the process that owns them, at 4 Hz, for every
    // publish rather than only the unrecorded ones. See
    // `docs/archive/2026-08-09-upload-readout-and-publish-reset.md` §5.

    // Safety backstop: refuse a deploy that would wipe most/all of the live
    // site. A near-total removal is the signature of an empty or stale build
    // manifest (the 2026-06-14 incident: a deleted hashes.json produced an
    // empty manifest → server diff said "remove everything" → 404), not a real
    // intent to delete the site. This guard works on the *outcome* (the diff),
    // so it catches the catastrophe regardless of which upstream path produced
    // the bad manifest — including ones the seal/fallback fixes don't foresee.
    // The guard filters `_moss/` rotations out of `remove` and, when the
    // server reports `server_total`, judges against the TRUE live file count
    // (2026-07-22: an OG-card rotation from a home-page rename false-blocked
    // a healthy publish).
    mass_remove::check_mass_removal_for_diff(
        manifest.len(),
        &diff,
        std::env::var("MOSS_DEPLOY_ALLOW_MASS_REMOVE").is_ok(),
    )?;

    // 8. Upload needed files
    let total = diff.need.len() as u32;
    let mp = MossPaths::new(folder_path);
    let site_dir = mp.generation_dir(sealed.generation_id());
    // Own the generation_id string once; every upload task clones it in.
    // This is the value threaded into X-Moss-Generation on sync, upload, and
    // chunked-create requests so the server routes all writes to the correct
    // generation directory — fixes the Stage 2 showstopper where the client
    // was syncing/uploading into generations/_default/ but committing to
    // generations/<16-hex>/ (an empty dir), causing a 422 completeness gate.
    let generation_id_str = sealed.generation_id().to_string();


    // Sum the bytes to upload (regular files only — symlinks transfer instantly
    // and would otherwise keep the byte total from ever being reached). One stat
    // per needed file: negligible vs the upload. (Design
    // docs/archive/2026-06-11-deploy-upload-progress.md §1b. file-count progress is
    // misleading for video-heavy sites — it hits ~97% before the videos start.)
    //
    // The same pass records each file's size, because the upload window admits
    // by BYTES (see deploy/upload.rs) and needs the size before it can decide
    // whether to let a task start. Symlinks count as 0: they transfer instantly.
    let mut upload_sizes: std::collections::HashMap<String, u64> =
        std::collections::HashMap::with_capacity(diff.need.len());
    let bytes_total: u64 = {
        let mut sum = 0u64;
        for p in &diff.need {
            let size = match tokio::fs::symlink_metadata(site_dir.join(p)).await {
                Ok(m) if !m.file_type().is_symlink() => m.len(),
                _ => 0,
            };
            upload_sizes.insert(p.clone(), size);
            sum += size;
        }
        sum
    };
    // Shared upload accounting: the per-file tasks update the atomics, a 4 Hz
    // ticker reads them and emits (so the nav-pill hairline fills by real bytes).
    let upload_state = std::sync::Arc::new(progress::UploadProgressState::new(
        bytes_total,
        total,
        diff.remove.len() as u32,
    ));

    // Resolve the site dir once; traversal check compares each file's
    // canonical path against this base. Previously resolved per-file inside
    // the spawn — one syscall per file for no benefit.
    let canonical_base = tokio::fs::canonicalize(&site_dir).await
        .map_err(|e| format!("Failed to resolve site dir: {}", e))?;

    // 4 Hz ticker: sample the atomics and emit a byte-based Uploading event so
    // the nav-pill hairline fills smoothly. The per-file tasks only touch the
    // atomics (no per-file emit), so the concurrent uploads don't flood the
    // event channel with bursts then go silent. (Design 2026-06-11 §"Ticker
    // pattern".) `interval` fires its first tick immediately → an initial
    // Uploading(0/total) event that transitions the panel into the upload stage.
    //
    // The handle is wrapped in an abort-on-drop guard: a tokio JoinHandle does
    // NOT abort on drop, so ANY early return below (e.g. the manifest-desync
    // `return Err` in the loop body) would otherwise leak a task emitting
    // Uploading forever. The guard aborts on every exit path; we ALSO abort it
    // explicitly after the loop so it stops before the Committing stage emit.
    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let ticker = {
        let st = std::sync::Arc::clone(&upload_state);
        let sink_t = std::sync::Arc::clone(sink);
        AbortOnDrop(tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
            loop {
                tick.tick().await;
                // Sampling is NOT a liveness bump: the ticker fires on a
                // timer and re-reports whether or not the counters moved, so a
                // bump here would advance the stall clock 4x/second for the
                // whole upload phase and leave the watchdog inert exactly where
                // stalls happen (`the_upload_progress_ticker_is_not_a_liveness_bump`).
                sink_t.upload_sample(&st);
            }
        }))
    };

    // A sliding window, not a batch barrier. `.chunks(20)` made every batch wait
    // for its slowest member, so one file on a slow uplink stalled nineteen that
    // were already done — and 20 concurrent uploads divided a 1.1 Mbps link into
    // slices too thin for any single request to finish inside Cloudflare's 125s
    // edge budget. Admission is by bytes and by count; see deploy/upload.rs and
    // crate::seta::upload_policy.
    let mut window = upload::UploadWindow::new();

    // How many files this deploy tolerates self-healing before treating the
    // pattern itself as evidence of a systemic problem (see
    // `upload::self_heal_cap`), and where the concurrent upload tasks below
    // report the corrected manifest entry for a file they self-healed —
    // `commit_sync` sends `manifest` verbatim as the server's new source of
    // truth, so a self-healed path's stale sealed entry must be corrected
    // here before that call, or the server's committed record is
    // permanently wrong for a file that was, in fact, uploaded correctly.
    let self_heal_cap = upload::self_heal_cap(diff.need.len());
    let self_heal_corrections: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<String, String>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

    for file_path in &diff.need {
        // Symlinks report 0 bytes, which is honest: they carry a short target
        // string, not file content.
        let file_size = upload_sizes.get(file_path).copied().unwrap_or(0);
        let admit_size = window.admission_cost(file_size);
        window.reserve(admit_size).await?;

        {
            let client = client.clone();
            let site_id = site_id.clone();
            let site_dir = site_dir.clone();
            let canonical_base = canonical_base.clone();
            let file_path = file_path.clone();
            let upload_state = std::sync::Arc::clone(&upload_state);
            let generation_id = generation_id_str.clone();
            let self_heal_corrections = std::sync::Arc::clone(&self_heal_corrections);
            // One link estimate per deploy, shared by every task in the window:
            // it routes single-PUT vs chunked and sizes each PATCH from what
            // this uplink is actually doing. See crate::seta::upload_policy.
            let throughput = window.throughput();
            // Look up the manifest entry to decide what kind of filesystem
            // object to upload. Mode lives in the manifest (server's source
            // of truth at sync/commit), but the upload PUT itself carries
            // the mode in the X-Moss-Entry header — see the deploy-upload
            // contract for why this duplication is intentional.
            //
            // diff.need is server-derived from the manifest we just POSTed,
            // so a path here that's missing from `manifest` indicates a
            // server-vs-client desync. Surface as an explicit error rather
            // than silently uploading whatever bytes are at the path.
            let entry_value = match manifest.get(&file_path) {
                Some(v) => v.clone(),
                None => {
                    return Err(format!(
                        "Path {} appeared in diff.need but not in client manifest",
                        file_path
                    ));
                }
            };

            window.spawn(admit_size, async move {
                use crate::types::content::{MODE_FILE, MODE_SYMLINK};
                // Enter the in-flight set BEFORE the first byte moves, so the
                // readout names a large file for the whole time it is uploading
                // rather than only after its first chunk lands.
                upload_state.begin_file(&file_path, admit_size);
                let (mode, expected_hash) = crate::types::content::parse_entry(&entry_value);
                match mode {
                    MODE_FILE => {
                        // Regular file: read bytes from disk, upload as bytes.
                        // Path-traversal check is over the canonicalized path
                        // (resolves any intermediate symlinks). Containment
                        // is enforced both server-side (writeFile) and here.
                        let full_path = site_dir.join(&file_path);
                        let canonical = tokio::fs::canonicalize(&full_path).await
                            .map_err(|e| {
                                if e.kind() == std::io::ErrorKind::NotFound {
                                    format!(
                                        "Manifest claims '{}' exists but it's missing on disk. \
                                         Your build cache (.moss/build.nosync/hashes.json) may be stale. \
                                         Try rebuilding the site. Underlying error: {}",
                                        file_path, e
                                    )
                                } else {
                                    format!("Failed to resolve {}: {}", file_path, e)
                                }
                            })?;
                        if !canonical.starts_with(&canonical_base) {
                            return Err(format!("Path traversal detected: {}", file_path));
                        }
                        // Size routing, integrity verification and the choice
                        // of single-PUT vs chunked all live in deploy/upload.rs
                        // — shared with deploy/prebuilt.rs, which had no size
                        // routing at all and would send a 100 MB video as one
                        // PUT. Manifest hashes here are xxh3_64.
                        let file_size = tokio::fs::metadata(&canonical).await
                            .map_err(|e| format!("Failed to stat {}: {}", file_path, e))?
                            .len();
                        let st_bytes = std::sync::Arc::clone(&upload_state);
                        let healed_hash = upload::upload_regular_file(
                            &client,
                            &site_id,
                            &file_path,
                            &canonical,
                            file_size,
                            &generation_id,
                            expected_hash,
                            upload::HashAlgo::Xxh3,
                            &throughput,
                            self_heal_cap,
                            Some(&move |n: u64| credit_upload_bytes(&st_bytes, n)),
                        )
                        .await?;
                        if let Some(actual_hash) = healed_hash {
                            self_heal_corrections.lock().unwrap().insert(
                                file_path.clone(),
                                crate::types::content::file_entry(&actual_hash),
                            );
                        }
                    }
                    MODE_SYMLINK => {
                        // Symlink: read the target via read_link (does NOT
                        // dereference), upload the target string. The site_dir
                        // join is purely to locate the link on disk; we don't
                        // canonicalize through it because the target may be a
                        // broken/forward-reference link that's still valid.
                        let full_path = site_dir.join(&file_path);
                        let target = tokio::fs::read_link(&full_path).await
                            .map_err(|e| format!("Failed to read symlink {}: {}", file_path, e))?;
                        let target_str = target.to_string_lossy().into_owned();
                        let computed_entry = crate::types::content::symlink_entry(&target_str);
                        if computed_entry != entry_value {
                            // Same self-heal discipline as upload.rs's
                            // file-hash check: settle, then re-read. A
                            // readlink is atomic, so this mainly catches a
                            // link still being re-pointed by a racing
                            // rebuild rather than a torn read.
                            tokio::time::sleep(upload::DRIFT_SETTLE_DELAY).await;
                            let resettled = tokio::fs::read_link(&full_path).await
                                .map_err(|e| format!("Failed to re-read symlink {}: {}", file_path, e))?;
                            let resettled_str = resettled.to_string_lossy().into_owned();
                            if resettled_str != target_str {
                                return Err(format!(
                                    "Deploy integrity error: symlink '{file_path}' target does \
                                     not match sealed manifest (still changing {:?} later — not \
                                     self-healing a moving target)",
                                    upload::DRIFT_SETTLE_DELAY
                                ));
                            }
                            upload::charge_self_heal(&throughput, self_heal_cap, &file_path)?;
                            // The target just read via read_link is the
                            // symlink's real current state, confirmed stable
                            // across a settle pause, so a stale sealed entry
                            // is logged and shipped anyway rather than
                            // failing the whole deploy (a synced vault can
                            // legitimately re-point a link between seal and
                            // upload).
                            log::warn!(
                                "[deploy] symlink '{file_path}' target does not match sealed \
                                 manifest (sealed {entry_value}, actual {computed_entry}) — \
                                 stable after a settle pause, uploading the actual target instead"
                            );
                            self_heal_corrections
                                .lock()
                                .unwrap()
                                .insert(file_path.clone(), computed_entry.clone());
                        }
                        client.upload_symlink(&site_id, &file_path, target_str, &generation_id)
                            .await
                            .map_err(|e| format!("Failed to upload symlink {}: {}", file_path, e))?;
                    }
                    other => {
                        return Err(format!(
                            "unknown manifest mode {} for {} — upgrade moss client",
                            other, file_path
                        ));
                    }
                }
                // The ticker owns the emit; each task only updates the shared
                // accounting (symlinks count toward the file total but added no
                // bytes above).
                upload_state.finish_file(&file_path);
                crate::infra::liveness::bump(); // a symlink's only proof of progress
                Ok::<(), String>(())
            });
        }
    }

    // Wait for the tail. The first failure has already aborted every remaining
    // task inside the window; the `?` here fails the deploy, and the ticker's
    // abort-on-drop guard stops it as this scope unwinds.
    window.drain().await?;

    // Upload finished: stop the ticker explicitly (before the Committing emit,
    // so no stray Uploading tick interleaves), then emit one final authoritative
    // Uploading event (the last 250 ms sample may be stale) so the hairline
    // reflects the true end state before Committing takes over.
    ticker.0.abort();
    sink.upload_sample(&upload_state);

    // Fold in any self-heals discovered during upload. `commit_sync` below
    // sends `manifest` verbatim as the server's new source of truth for this
    // generation — it has no independent way to check these hashes — so a
    // stale sealed entry left uncorrected here would commit a permanently
    // wrong record for a file that was actually uploaded correctly.
    {
        let corrections = self_heal_corrections.lock().unwrap();
        if !corrections.is_empty() {
            log::info!(
                "[deploy] correcting {} manifest {} to actually-shipped hashes before commit",
                corrections.len(),
                if corrections.len() == 1 { "entry" } else { "entries" }
            );
        }
        for (path, entry) in corrections.iter() {
            manifest.insert(path.clone(), entry.clone());
        }
    }

    // 9. Commit
    sink.stage(progress::DeployStage::Committing, 0, 0, "Making changes live...");
    log::info!(
        "deploy: shipping generation {} ({} files)",
        sealed.generation_id(),
        manifest.len()
    );
    let commit_result = client
        .commit_sync(&site_id, &manifest, sealed.generation_id())
        .await
        .map_err(|e| e.to_string())?;

    // Record what just went live, so the NEXT publish can say what it will
    // change instead of showing a flat file count.
    //
    // Placement is the whole correctness argument. This sits after
    // `commit_sync` returned 200 and after the skew check, so no record is
    // written for a publish that did not land — everything before this point
    // `?`-propagates. Writing it earlier, or on a failed commit, would make the
    // next change set under-report: it would tell the author nothing will
    // change when something will, which is the one direction of wrongness they
    // cannot correct from the UI.
    //
    // Best effort in the other direction: a failed write costs one degraded
    // (unclassified) change set, never a failed deploy, so it warns like
    // `record_publish` below rather than propagating. Shared with
    // the plugin publish path — see `deploy::landed`.
    //
    // Also the source of the completion-scoped page rows the verification
    // burst below names (task 4-6) — `record_landed` returns the same
    // `PageChangeSummary` it hands `after_landing`, so this must run BEFORE
    // the burst rather than after it (the order task 4-3 originally landed
    // in, when nothing downstream needed the summary yet).
    let page_summary = if let Some(target) = crate::config::deployment::slot_for("moss", Some(&site_id)) {
        let history = crate::deploy::history::HistoryStore::in_vault(folder_path);
        landed::record_landed(folder_path, sealed, &target, ports, &history).await
    } else {
        crate::deploy::change_record::PageChangeSummary::default()
    };

    // One-shot verification burst: does the origin now say this generation
    // is live, and does the public address reach the site (and, task 4-6,
    // each named page's URL). This supersedes the ad-hoc skew log it
    // replaces — `ControlProbe::ServingOlderGeneration` is the same "server
    // is on a different generation" comparison, now folded into a real
    // verdict instead of a diagnostic-only warning. Fire-and-forget by the
    // port's own contract (see `DeployPorts::begin_moss_verification`): this
    // await returns once the implementation has *started* the burst, never
    // once it has finished, so it cannot hold up `push_site`'s own return.
    ports
        .begin_moss_verification(
            identity,
            environment.clone(),
            &site_id,
            &folder_path_str,
            sealed.generation_id(),
            &page_summary,
        )
        .await;

    // The commit returned 200: the site is LIVE. From here a stall cancel would
    // tell the user their publish failed when it succeeded, so every step below
    // re-proves liveness explicitly — including inside the loops we cannot see
    // the end of. Nothing may be judged by silence it could not have broken.
    crate::infra::liveness::bump();

    // 9b. Enter the post-commit bookkeeping phase. These steps (subscribers
    //     sync, analytics sync, deployment info save,
    //     domain orchestrator) used to run silently for several seconds on
    //     sites with many subscribers or first-time domain setup, leaving
    //     the progress panel stuck on "Making changes live" long after the
    //     commit had actually landed. Surface them under a single
    //     `Finalizing` stage so the user sees motion.
    sink.stage(progress::DeployStage::Finalizing, 0, 0, "Finalizing...");

    // Step 10 stood here: an authenticated `pull_site_data` whose response was
    // discarded, kept alive only to gate the legacy `site-data.json` sweep that
    // followed it. The sweep reads `.moss/data/` and nothing else, so the
    // request answered no question — and `pull_site_data_if_hosted_impl` runs
    // the same sweep on every project open, where a one-time pre-CSV migration
    // belongs. Deleted at C4e rather than carried into the open binary, which
    // would have paid a network round-trip per publish for it.

    // 10b. Sync raw analytics events — non-fatal, best-effort.
    // Acquire the per-site mutex so this post-publish sync cannot race with a
    // concurrent panel-open sync_analytics_events command on the same site.
    {
        // `sync_events` is an iteration-unbounded pagination loop
        // (SYNC_PAGE_LIMIT = 50_000, vault/analytics.rs) of un-retried 120s
        // calls, so two pages back-to-back would outlast STALL_TIMEOUT on their
        // own. It bumps per page, so silence here means a genuine hang and is
        // judged like every other phase — the site being live already is not a
        // reason to leave the user watching a publish that never returns.
        let _guard = events_lock.lock().await;
        if let Err(e) = crate::vault::analytics::sync_events(&client, folder_path, &site_id).await {
            log::warn!(target: "deploy", "events sync after publish failed: {}", e);
        }
    }

    // 10c. What the next build's rename detection diffs against used to be a
    // second file taken here, and only here — so a site published through a
    // deploy plugin never got one. It is now a field on the publish record that
    // `landed::record_what_is_live` fills, which both publish paths reach: on
    // this one from `record_landed` above, still after `commit_sync` and over
    // the same bytes, since nothing between the two writes the article map.
    crate::infra::liveness::bump();

    // 11. Save deployment info to config with DnsTarget for moss hosting.
    // Both apex (@) and www A-records point to the VPS (`MOSSPUB_VPS_IP`). The
    // www-preferred architecture enables future CDN integration (www can CNAME
    // to a CDN, apex cannot per DNS RFC).
    let moss_dns_target = Some(crate::plugins::types::DnsTarget {
        records: vec![
            crate::plugins::types::DnsRecord {
                record_type: "A".to_string(),
                name: "@".to_string(),
                value: MOSSPUB_VPS_IP.to_string(),
                ttl: Some(3600),
            },
            crate::plugins::types::DnsRecord {
                record_type: "A".to_string(),
                name: "www".to_string(),
                value: MOSSPUB_VPS_IP.to_string(),
                ttl: Some(3600),
            },
        ],
    });

    // Use the env-derived URL rather than the server-echoed one so staging
    // deploys surface the correct staging URL (not the prod-shaped echo).
    // Mirrors the already-fixed short-circuit paths at :324/:346.
    //
    // This is the SUBDOMAIN url on purpose: `last_deployment_url` must stay
    // the original serving URL even when a custom domain exists (the publish
    // panel's "Also redirects from …" caption derives from it — see
    // `build_deployment_status`). The user-facing success URL is resolved
    // separately below, after the domain orchestrator has run.
    let subdomain_url = format!("https://{}{}", site_id, environment.site_suffix());
    let deployed_at = chrono::DateTime::from_timestamp(commit_result.timestamp as i64, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    if let Err(e) = crate::vault::deployment_state::record_publish(
        &folder_path_str,
        crate::vault::deployment_state::PublishOutcome {
            url: subdomain_url,
            deployed_at,
            dns_target: moss_dns_target,
            generation_id: Some(sealed.generation_id().to_string()),
            // No site_url: moss derives it from site_id at build time.
            ..Default::default()
        },
    ) {
        log::warn!("Failed to save deployment info: {}", e);
    }

    // 12. Trigger domain orchestrator (same as deploy_site).
    // Bounded by the orchestrator's own 60s safety net, well inside
    // STALL_TIMEOUT — one bump on each side is enough.
    crate::infra::liveness::bump();
    ports.ensure_domain_ready(&folder_path_str).await;
    crate::infra::liveness::bump();

    // 13. Complete is emitted by `with_publish_guard` on the way out, so every
    // publish ends the same way whoever shipped the bytes. `total, total` is
    // not lost information: neither listener reads the counts once the stage is
    // terminal.

    // Resolved AFTER the orchestrator so a custom domain verified during
    // this very deploy already wins: the "View site" toast, the CLI success
    // line, and the syndication walk all receive the address the user
    // actually publishes under, not the mosspub subdomain.
    let url = view_site_url(&folder_path_str, &site_id, environment);

    Ok(PushResult::Success {
        url,
        files_uploaded: commit_result.files_updated,
        files_removed: commit_result.files_removed,
    })
}


/// One moss-hosted publish, from a folder to a result — the build included.
///
/// The twin of [`super::prebuilt::run_prebuilt_deploy`] for the route that
/// needs moss's own build, and the whole of what `moss deploy <folder>` does in
/// a process with no window. The app does not call this: it publishes from a
/// manifest a watcher already sealed, which is the resolution
/// `src-tauri/src/deploy.rs` keeps.
///
/// The build is synchronous and one-shot (`exits_after_build`), so the seal
/// happens inline and [`super::one_shot::build_and_seal`] has the manifest by the time
/// it returns. That last clause was false when this function was
/// first written: the inline seal arm hardcoded `LogAnnouncer` and discarded
/// `host.announcer`, so every hosted deploy failed with "the build sealed no
/// generation" while a unit test of the wrapper passed. Both arms now use the
/// host's own announcer, and
/// [`tests::a_one_shot_build_hands_its_manifest_to_the_host_announcer`] drives
/// the pipeline rather than the wrapper, because a test that constructs its own
/// collaborator cannot see that nobody calls it. A build that produced no seal is a build that did
/// not materialize a generation, and there is nothing to publish; that is an
/// error rather than an empty publish, because an empty publish would remove
/// every file from the live site.
///
/// `host_ports` is a factory for the same reason [`crate::ops::HeadlessBuildRun`]
/// takes one: the ports are constructed after the folder session is registered,
/// and they are not `Clone`.
///
/// Warnings do not stop the publish, and that is deliberate parity rather than
/// leniency: the app's own `moss deploy` builds and publishes whatever the
/// build produced, and the one hard refusal on both sides is
/// [`crate::deploy::refuse_publish`]. A binary that refused here on a problem
/// count the app publishes through would be the divergence this whole track
/// exists to delete. `--strict` belongs to `build`, where the caller decides.
///
/// One refusal is the terminal's alone, on purpose: a folder behind the live
/// site, lifted by `overwrite_newer` — see [`crate::deploy::resolve_publish_inputs`].
pub async fn run_hosted_deploy(
    folder: &Path,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
    requested_site_id: Option<&str>,
    overwrite_newer: bool,
    sink: &std::sync::Arc<dyn progress::DeploySink>,
) -> Result<PushResult, String> {
    let folder_str = folder.to_string_lossy().to_string();

    // Before the build, not after: a folder with no site cannot publish however
    // well it builds, and an evicted identity key is knowable at t=0 — asking
    // for it is usually what makes it arrive.
    let Some((site_id, identity)) =
        crate::deploy::resolve_publish_inputs(folder, requested_site_id, overwrite_newer, sink).await?
    else {
        return Ok(PushResult::NeedsSetup);
    };

    // The stage-write lock every concurrent-build path takes. `run_pipeline`
    // reaches for it, and so does the seal tail.
    let _session = crate::system::folder_session::register_session(&folder_str);

    sink.stage(progress::DeployStage::Preparing, 0, 0, "Building the site...");

    let root = crate::vault::paths::VaultRoot::resolve(folder);
    let host = host_ports(&folder_str);
    let taken = super::one_shot::build_and_seal(&root, host, plugins).await?;

    // Only after the build: the verdict this reads is the build's own, and
    // asking before it would refuse on the previous run's answer or on none.
    crate::deploy::refuse_publish(&folder_str)?;

    let sealed = super::one_shot::require_sealed(taken)?;

    let ports = super::one_shot::HeadlessDeployPorts;
    let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    let cx = PushContext {
        folder_path: folder,
        identity: &identity,
        site_id: &site_id,
        sink,
        ports: &ports,
        events_lock: &events_lock,
    };
    push_site_inner(&sealed, &cx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::progress::DeploySink;

    /// The upload phase's byte credit is a liveness bump. If this stops being
    /// true, a publish that is uploading steadily for longer than STALL_TIMEOUT
    /// gets cancelled while healthy — the 2026-08-04 failure, reintroduced.
    #[test]
    fn upload_byte_credit_is_a_liveness_bump() {
        let st = progress::UploadProgressState::new(4096, 1, 0);
        let before = crate::infra::liveness::thread_bump_count();
        credit_upload_bytes(&st, 1024);
        assert_eq!(
            st.bytes_uploaded.load(std::sync::atomic::Ordering::Relaxed),
            1024
        );
        assert!(
            crate::infra::liveness::thread_bump_count() > before,
            "confirmed bytes are the upload phase's proof of progress"
        );
    }

    /// The other direction, and the one an adversarial review found first: the
    /// 250 ms progress ticker re-emits whether or not the counters moved. Bumping
    /// from it would advance the clock 4x/second for the whole upload phase and
    /// leave the watchdog inert exactly where stalls happen.
    #[test]
    fn the_upload_progress_ticker_is_not_a_liveness_bump() {
        let st = progress::UploadProgressState::new(4096, 8, 0);
        let sink = progress::RecordingSink::default();
        let before = crate::infra::liveness::thread_bump_count();
        for _ in 0..40 {
            sink.upload_sample(&st);
        }
        assert_eq!(sink.drain().len(), 40, "every tick reported, none credited");
        assert_eq!(
            crate::infra::liveness::thread_bump_count(),
            before,
            "the ticker fires on a timer, not on progress — it must never bump"
        );
    }

    #[test]
    fn deploy_status_label_maps_outcomes() {
        let ok = Ok(PushResult::Success {
            url: "https://x.mosspub.com".to_string(),
            files_uploaded: 1,
            files_removed: 0,
        });
        assert_eq!(deploy_status_label(&ok), "success");
        assert_eq!(deploy_status_label(&Ok(PushResult::NeedsSetup)), "needs_setup");
        let err: Result<PushResult, String> = Err("boom".to_string());
        assert_eq!(deploy_status_label(&err), "failed");
    }

    // ── the moss-hosting verification burst starts after commit ────────────

    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::ports::deploy::DeployPorts;
    use crate::build::served_path::ServedPath;
    use crate::config::environment::HostingEnvironment;
    use crate::types::content::SiteHashes;

    /// Answers exactly `responses.len()` sequential TCP connections in
    /// order, then stops accepting — a call beyond that gets a fast
    /// connection-refused, which every caller past `commit_sync` in
    /// `push_site_inner_impl` already treats as best-effort (e.g.
    /// `vault::analytics::sync_events`'s non-fatal warn). Drains each
    /// request fully (by `Content-Length`) before responding, so a POST body
    /// still in flight cannot race the connection close. `Connection: close`
    /// on every canned response forces a fresh TCP connection per request
    /// rather than a pooled one this single-shot listener cannot serve.
    /// Drain-request-then-canned-response is `crate::test_mock_http_conn` —
    /// shared with `seta::chunked_upload_tests` and `deploy::upload::tests`,
    /// which had each grown their own copy of this exact loop. Pattern from
    /// `seta::chunked_upload_tests::upload_file_chunked_sends_content_hash_header`.
    async fn mock_seta_sequence(responses: Vec<&'static [u8]>) -> std::net::SocketAddr {
        let (addr, _last_request) = mock_seta_sequence_capturing(responses).await;
        addr
    }

    /// Same as [`mock_seta_sequence`], but also hands back the LAST call's
    /// raw request bytes — for a test that needs to inspect what that call
    /// actually sent, e.g. `commit_sync`'s manifest body after a self-heal.
    async fn mock_seta_sequence_capturing(
        responses: Vec<&'static [u8]>,
    ) -> (std::net::SocketAddr, tokio::sync::oneshot::Receiver<Vec<u8>>) {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let last = responses.len().saturating_sub(1);
            let mut tx = Some(tx);
            for (i, resp) in responses.into_iter().enumerate() {
                let (stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => return,
                };
                let raw = crate::test_mock_http_conn(stream, resp).await;
                if i == last {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(raw);
                    }
                }
            }
        });

        (addr, rx)
    }

    /// One page, `index.html` — enough for `push_site_inner_impl` to reach
    /// `commit_sync` while the sync response below reports nothing to
    /// upload, so no bytes for it are ever read off disk.
    fn sealed_fixture() -> SealedManifest {
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = ServedPath::from_source("index.html").unwrap();
        pending.register(&sp, b"<html>Home</html>", HashBucket::Files);
        pending.seal()
    }

    /// One recorded `begin_moss_verification` call.
    struct VerificationCall {
        site_id: String,
        folder_path: String,
        generation_id: String,
        summary: crate::deploy::change_record::PageChangeSummary,
    }

    /// One event `SpyPorts` observed, in the order it happened — the shape
    /// `commit_sync_success_calls_begin_moss_verification` needs to assert
    /// task 4-6's ordering (`after_landing` before the verification call),
    /// not just each call's own arguments in isolation.
    enum SpyEvent {
        Landed { target: String },
        Verification(VerificationCall),
    }

    /// Records every `after_landing`/`begin_moss_verification` call, in
    /// order; every other port method is a one-line no-op, same shape as
    /// `HeadlessDeployPorts`.
    #[derive(Default)]
    struct SpyPorts {
        events: std::sync::Mutex<Vec<SpyEvent>>,
    }

    #[async_trait::async_trait]
    impl DeployPorts for SpyPorts {
        fn pin_generation_raw(&self, _generation_id: &str) {}
        fn unpin_generation(&self, _generation_id: &str) {}
        async fn after_landing(&self, _folder: &Path, target: &str, _summary: &crate::deploy::change_record::PageChangeSummary) {
            self.events.lock().unwrap().push(SpyEvent::Landed { target: target.to_string() });
        }
        async fn ensure_domain_ready(&self, _folder_path: &str) {}
        async fn begin_moss_verification(
            &self,
            _identity: &Identity,
            _environment: HostingEnvironment,
            site_id: &str,
            folder_path: &str,
            generation_id: &str,
            summary: &crate::deploy::change_record::PageChangeSummary,
        ) {
            self.events.lock().unwrap().push(SpyEvent::Verification(VerificationCall {
                site_id: site_id.to_string(),
                folder_path: folder_path.to_string(),
                generation_id: generation_id.to_string(),
                summary: summary.clone(),
            }));
        }
    }

    /// The row's own test: `push_site_inner` must call
    /// `ports.begin_moss_verification(...)` once `commit_sync` has
    /// succeeded AND `record_landed` (`after_landing`) has already run —
    /// task 4-6 moved the burst past landing so it can carry the same
    /// `PageChangeSummary` landing computed, never a second copy — carrying
    /// this publish's generation id and that summary. Ablation lives at the
    /// call site in `push_site_inner_impl` — comment it out and this
    /// assertion goes red with zero recorded calls.
    #[tokio::test]
    async fn commit_sync_success_calls_begin_moss_verification() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();

        let addr = mock_seta_sequence(vec![
            // 1. The pre-existing get_live_generation short-circuit check:
            //    404 -> Ok(None), so the normal sync/upload/commit path runs.
            b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            // 2. sync_manifest: nothing to upload, one stale page to remove
            //    (well under the mass-removal guard's floor, so it passes).
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 35\r\n\r\n{\"need\":[],\"remove\":[\"stale.html\"]}",
            // 3. commit_sync: the site is live.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{\"url\":\"https://verify-test.mosspub.com\",\"files_updated\":0,\"files_removed\":1,\"timestamp\":1700000000}",
        ])
        .await;
        std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));

        let identity = Identity::generate().expect("generate identity");
        let sealed = sealed_fixture();
        let dir = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(dir.path());
        std::fs::create_dir_all(mp.generation_dir(sealed.generation_id())).unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(),
            identity: &identity,
            site_id: "verify-test",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };

        let result = push_site_inner(&sealed, &cx).await;

        match prev_url {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }

        assert!(
            result.is_ok(),
            "publish should succeed against the mock server: {result:?}"
        );

        let events = spy.events.lock().unwrap();
        assert_eq!(
            events.len(),
            2,
            "landing then the verification burst, exactly once each: {}",
            events.len()
        );
        let SpyEvent::Landed { target } = &events[0] else {
            panic!("first event must be record_landed's after_landing, not the verification burst")
        };
        assert_eq!(target, "moss:verify-test", "record_landed's own target naming");
        let SpyEvent::Verification(call) = &events[1] else {
            panic!("second event must be the verification burst, after landing")
        };
        assert_eq!(call.site_id, "verify-test");
        assert_eq!(call.folder_path, dir.path().to_string_lossy().to_string());
        assert_eq!(call.generation_id, sealed.generation_id());
        assert_eq!(
            call.summary,
            crate::deploy::change_record::PageChangeSummary::default(),
            "no article map on disk in this fixture — the same summary record_landed \
             returned and handed to after_landing, not a second, independently computed one"
        );
    }

    /// A site whose `.moss/config.toml` was last saved by a newer moss must
    /// never publish: the build behind this sealed manifest already rendered
    /// with every setting this app doesn't recognize the shape of at its
    /// silent default (`ConfigFile::parse`'s `VersionAhead` fallback), and
    /// shipping that to the live site is not recoverable the way a stale
    /// preview is. `MOSS_SETA_URL` points at a closed local port — reserved
    /// and released before the test, guaranteed free, never production — as a
    /// belt-and-suspenders check that the refusal fires before ANY network
    /// call: if it didn't, this would hang or error on connection-refused
    /// instead of returning the named error, and `spy.events` would be
    /// non-empty. Ablated by deleting the version-ahead check at the top of
    /// `push_site_inner_impl`: goes red as `result.is_ok()` (it reaches the
    /// closed port, gets connection-refused, and — worse — every event that
    /// mattering here is the ABSENCE of network activity, not its shape).
    #[tokio::test]
    async fn refuses_to_publish_a_version_ahead_config() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();

        // Reserve a free port, then release it: nothing listens there, so any
        // connection attempt fails fast with connection-refused rather than
        // hanging or reaching a real host.
        let closed_addr = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        std::env::set_var("MOSS_SETA_URL", format!("http://{closed_addr}"));

        let identity = Identity::generate().expect("generate identity");
        let sealed = sealed_fixture();
        let dir = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(dir.path());
        std::fs::create_dir_all(mp.generation_dir(sealed.generation_id())).unwrap();
        let moss_dir = dir.path().join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        std::fs::write(
            moss_dir.join("config.toml"),
            format!("schema_version = {}\n", crate::config::migrations::CURRENT_VERSION + 1),
        )
        .unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(),
            identity: &identity,
            site_id: "version-ahead-test",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };

        let result = push_site_inner(&sealed, &cx).await;

        match prev_url {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }

        let err = result.expect_err("publish must be refused, not attempted");
        assert!(err.contains("schema_version") && err.contains("newer"), "got: {err}");
        assert!(
            spy.events.lock().unwrap().is_empty(),
            "no deploy port should have been touched — the refusal must come before any network activity"
        );
    }

    /// The symlink-integrity check's counterpart to `upload.rs`'s file-hash
    /// self-heal: a synced vault can legitimately re-point a symlink between
    /// seal and upload the same way it can rewrite a raw asset's bytes, and
    /// `target_str` here is already the link's real current target (just read
    /// via `read_link`). A stale sealed entry must warn and ship it, not abort
    /// the whole deploy the way the old hard `return Err` did.
    #[tokio::test]
    async fn a_drifted_symlink_target_self_heals_instead_of_failing_the_deploy() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();

        let addr = mock_seta_sequence(vec![
            // 1. get_live_generation short-circuit: 404 -> Ok(None).
            b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            // 2. sync_manifest: one symlink needs uploading.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 29\r\n\r\n{\"need\":[\"link\"],\"remove\":[]}",
            // 3. upload_symlink's PUT.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            // 4. commit_sync.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 101\r\n\r\n{\"url\":\"https://symlink-test.mosspub.com\",\"files_updated\":1,\"files_removed\":0,\"timestamp\":1700000000}",
        ])
        .await;
        std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));

        // Sealed manifest carries a symlink entry whose target hash is stale,
        // as if a background rebuild re-pointed the link after this manifest
        // was sealed.
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = ServedPath::from_source("link").unwrap();
        pending.register_hashed(
            &sp,
            &crate::types::content::symlink_entry("stale-target"),
            HashBucket::Files,
        );
        let sealed = pending.seal();

        let identity = Identity::generate().expect("generate identity");
        let dir = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(dir.path());
        let gen_dir = mp.generation_dir(sealed.generation_id());
        std::fs::create_dir_all(&gen_dir).unwrap();
        // The real, current target on disk — what a racing rebuild left behind
        // after the manifest above was sealed against "stale-target".
        std::os::unix::fs::symlink("current-target", gen_dir.join("link")).unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(),
            identity: &identity,
            site_id: "symlink-test",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };

        let result = push_site_inner(&sealed, &cx).await;

        match prev_url {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }

        assert!(
            result.is_ok(),
            "a drifted symlink target must self-heal, not fail the whole deploy: {result:?}"
        );
    }

    /// The stability guard and the manifest correction, proven together: the
    /// server has no independent source of truth for the hashes it commits —
    /// `commit_sync`'s POST body IS `{"manifest": <path→hash>, ...}` — so a
    /// self-healed file whose manifest entry is left uncorrected commits a
    /// permanently wrong record for a file that was, in fact, uploaded
    /// correctly. Ablated by removing the `self_heal_corrections` fold-in
    /// before the commit call: this goes red with the STALE sealed hash
    /// still present in the committed body instead of the real one.
    #[tokio::test]
    async fn a_self_healed_file_corrects_its_manifest_entry_before_commit() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();

        let real_bytes = b"<html>Real, current content the sealed manifest never saw</html>";
        let real_hash = crate::build::assets::paths::compute_binary_hash(real_bytes);
        let stale_hash = "0000000000000000";
        assert_ne!(real_hash, stale_hash, "fixture sanity: the drift must be real");

        let (addr, commit_rx) = mock_seta_sequence_capturing(vec![
            // 1. get_live_generation short-circuit: 404 -> Ok(None).
            b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            // 2. sync_manifest: the drifted file needs uploading.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 35\r\n\r\n{\"need\":[\"index.html\"],\"remove\":[]}",
            // 3. the file's PUT.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            // 4. commit_sync — captured below.
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{\"url\":\"https://healed-test.mosspub.com\",\"files_updated\":1,\"files_removed\":0,\"timestamp\":1700000000}",
        ])
        .await;
        std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));

        // Sealed manifest carries a stale hash for "index.html" — as if a
        // background rebuild rewrote the file's real bytes after this
        // manifest was sealed.
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = ServedPath::from_source("index.html").unwrap();
        pending.register_hashed(
            &sp,
            &crate::types::content::file_entry(stale_hash),
            HashBucket::Files,
        );
        let sealed = pending.seal();

        let identity = Identity::generate().expect("generate identity");
        let dir = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(dir.path());
        let gen_dir = mp.generation_dir(sealed.generation_id());
        std::fs::create_dir_all(&gen_dir).unwrap();
        // The real, current bytes on disk — what a racing rebuild actually
        // wrote after the manifest above was sealed against `stale_hash`.
        std::fs::write(gen_dir.join("index.html"), real_bytes).unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(),
            identity: &identity,
            site_id: "healed-test",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };

        let result = push_site_inner(&sealed, &cx).await;

        match prev_url {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }

        assert!(
            result.is_ok(),
            "a hash drift stable across the settle pause must self-heal, not fail the deploy: {result:?}"
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
            body.contains(&real_hash),
            "commit_sync's manifest must carry the hash actually shipped for the self-healed \
             file, not the stale sealed one: {body}"
        );
        assert!(
            !body.contains(stale_hash),
            "the stale sealed hash must not survive into the committed manifest: {body}"
        );
    }

    // ── a copy behind the live site ────────────────────────────────────────
    //
    // Both `moss deploy` routes to moss hosting are here, prebuilt included:
    // the guard is one call in `deploy::resolve_publish_inputs`, which both
    // reach, and they share these fixtures.

    /// A canned `200` with a JSON body, leaked for `mock_seta_sequence`.
    fn json_200(body: &str) -> &'static [u8] {
        let response = format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        Box::leak(response.into_bytes().into_boxed_slice())
    }

    /// What the server says after another copy published at 2026-09-24 14:03
    /// UTC — after this folder's own last publish.
    const PUBLISHED_ELSEWHERE_SINCE: &str = r#"{"generation_id":"theirs","deployed_at":1790258580}"#;

    /// A one-page folder whose record says it last published generation
    /// `ours` at 2026-09-20 09:12 UTC. Under the workspace `target/` like
    /// `one_shot`'s pipeline test, because the override test builds it.
    fn folder_that_last_published(record: bool) -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/hosted");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new().prefix("behind").tempdir_in(&base).unwrap();
        std::fs::write(tmp.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
        std::fs::create_dir_all(tmp.path().join(".moss")).unwrap();
        std::fs::write(tmp.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"behind-test\"\n").unwrap();
        if record {
            crate::build::manifest::published_record::save(
                &MossPaths::new(tmp.path()),
                &crate::build::manifest::change_set::PublishedSnapshot {
                    generation_id: "ours".to_string(),
                    target: "moss:behind-test".to_string(),
                    published_at: "2026-09-20T09:12:30+00:00".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        tmp
    }

    /// `run_hosted_deploy` exactly as `moss deploy` calls it, minus plugins.
    async fn cli_deploy(folder: &Path, overwrite_newer: bool) -> Result<PushResult, String> {
        run_hosted_deploy(
            folder,
            &crate::cli::host::cli_host_ports,
            crate::build::PluginMode::Skip,
            None,
            overwrite_newer,
            &progress::silent(),
        )
        .await
    }

    /// Awaits `publish` against the mock at `addr`. The caller holds
    /// `ENV_TEST_MUTEX`; the client reads the variable when the future runs.
    async fn with_seta_url<F: std::future::Future>(addr: std::net::SocketAddr, publish: F) -> F::Output {
        let prev = std::env::var("MOSS_SETA_URL").ok();
        std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
        let out = publish.await;
        match prev {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }
        out
    }

    /// The near-miss this guards: a folder behind the copy that last published
    /// would have replaced the newer live site. The server answers exactly one
    /// request, the live-generation probe; the refusal must come from it, before
    /// the build and before anything is uploaded. Ablated by deleting the
    /// `refuse_stale_copy` call in `run_hosted_deploy`: the deploy goes on to
    /// build and fails at the sync with a connection error instead.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cli_deploy_from_a_copy_behind_the_live_site_uploads_nothing() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let (addr, probe) = mock_seta_sequence_capturing(vec![json_200(PUBLISHED_ELSEWHERE_SINCE)]).await;

        let result = with_seta_url(addr, cli_deploy(folder.path(), false)).await;

        let err = result.expect_err("a copy behind the live site must not publish");
        assert!(err.contains("published from another copy"), "got: {err}");
        let probe = String::from_utf8_lossy(&probe.await.expect("the probe is the one request made")).to_string();
        assert!(probe.starts_with("GET /api/sites/behind-test/generation"), "got: {probe}");
    }

    /// `--overwrite-newer` publishes over it: the build runs and the push
    /// reaches the sync, which reports nothing to change.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_override_publishes_over_a_newer_live_site() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let addr = mock_seta_sequence(vec![
            // push's own no-op probe: a different generation, so it goes on.
            json_200(PUBLISHED_ELSEWHERE_SINCE),
            json_200(r#"{"need":[],"remove":[]}"#),
        ])
        .await;

        let result = with_seta_url(addr, cli_deploy(folder.path(), true)).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
    }

    /// The app publishes through `push_site_inner`, and its behaviour is
    /// unchanged: the same folder and server state go ahead there.
    #[tokio::test]
    async fn the_app_publish_path_does_not_refuse_a_copy_behind_the_live_site() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let sealed = sealed_fixture();
        std::fs::create_dir_all(MossPaths::new(folder.path()).generation_dir(sealed.generation_id())).unwrap();
        let addr = mock_seta_sequence(vec![
            json_200(PUBLISHED_ELSEWHERE_SINCE),
            json_200(r#"{"need":[],"remove":[]}"#),
        ])
        .await;

        let identity = Identity::generate().expect("generate identity");
        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: folder.path(),
            identity: &identity,
            site_id: "behind-test",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };
        let result = with_seta_url(addr, push_site_inner(&sealed, &cx)).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
    }

    /// No record means a first publish from this copy: nothing to compare,
    /// so no refusal whatever the server says.
    #[tokio::test]
    async fn a_folder_with_no_publish_record_is_not_refused() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(false);
        let addr = mock_seta_sequence(vec![json_200(PUBLISHED_ELSEWHERE_SINCE)]).await;
        let identity = Identity::generate().unwrap();

        let result = with_seta_url(addr, crate::deploy::refuse_stale_copy(folder.path(), "behind-test", &identity)).await;

        assert_eq!(result, Ok(()));
    }

    /// A probe that fails is not evidence: the deploy goes ahead, as it does
    /// past `push`'s own no-op check.
    #[tokio::test]
    async fn a_failed_probe_does_not_refuse() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let addr = mock_seta_sequence(vec![
            b"HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        ])
        .await;
        let identity = Identity::generate().unwrap();

        let result = with_seta_url(addr, crate::deploy::refuse_stale_copy(folder.path(), "behind-test", &identity)).await;

        assert_eq!(result, Ok(()));
    }

    /// A prebuilt tree another tool produced, for `--prebuilt`.
    fn prebuilt_site() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/hosted");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new().prefix("prebuilt").tempdir_in(&base).unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>Built elsewhere</html>").unwrap();
        dir
    }

    /// The same near-miss through `--prebuilt`, which publishes to the same
    /// site. Ablated by deleting the `refuse_stale_copy` call in
    /// `resolve_publish_inputs`: the deploy goes on to hash and sync instead.
    #[tokio::test]
    async fn a_prebuilt_deploy_from_a_copy_behind_the_live_site_uploads_nothing() {
        let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let site = prebuilt_site();
        let (addr, probe) = mock_seta_sequence_capturing(vec![json_200(PUBLISHED_ELSEWHERE_SINCE)]).await;

        let result = with_seta_url(
            addr,
            crate::deploy::prebuilt::run_prebuilt_deploy(folder.path(), site.path(), None, false, &progress::silent()),
        )
        .await;

        let err = result.expect_err("a copy behind the live site must not publish");
        assert!(err.contains("published from another copy"), "got: {err}");
        let probe = String::from_utf8_lossy(&probe.await.expect("the probe is the one request made")).to_string();
        assert!(probe.starts_with("GET /api/sites/behind-test/generation"), "got: {probe}");
    }

    /// A folder that publishes with `--prebuilt` and then with moss's own
    /// build is the same copy both times, so the second deploy must not read
    /// the first as someone else's. Ablated by deleting the
    /// `record_prebuilt_landed` call in `push_prebuilt_inner`: the record
    /// stays at the earlier moss-built publish and the hosted deploy is
    /// refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn after_a_prebuilt_publish_the_next_hosted_deploy_is_not_refused() {
        let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let site = prebuilt_site();
        let prebuilt_gen = crate::build::assets::paths::compute_manifest_generation_id(
            &crate::deploy::prebuilt::build_manifest_from_dir(site.path()).unwrap(),
        );
        let live_prebuilt: &'static str =
            Box::leak(format!(r#"{{"generation_id":"{prebuilt_gen}","deployed_at":1790258580}}"#).into_boxed_str());

        let addr = mock_seta_sequence(vec![
            // The guard: this copy's own generation is live, so it goes ahead.
            json_200(r#"{"generation_id":"ours","deployed_at":1790000000}"#),
            // push_prebuilt's no-op probe, the sync, the upload, the commit.
            json_200(r#"{"generation_id":"ours","deployed_at":1790000000}"#),
            json_200(r#"{"need":["index.html"],"remove":[]}"#),
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            json_200(&format!(
                r#"{{"url":"https://behind-test.mosspub.com","files_updated":1,"files_removed":0,"timestamp":1790258580,"generation_id":"{prebuilt_gen}"}}"#
            )),
        ])
        .await;
        let published = with_seta_url(
            addr,
            crate::deploy::prebuilt::run_prebuilt_deploy(folder.path(), site.path(), None, false, &progress::silent()),
        )
        .await;
        assert!(matches!(published, Ok(PushResult::Success { files_uploaded: 1, .. })), "got: {published:?}");

        let addr = mock_seta_sequence(vec![
            // The guard, then push's own probe: the prebuilt tree is live.
            json_200(live_prebuilt),
            json_200(live_prebuilt),
            json_200(r#"{"need":[],"remove":[]}"#),
        ])
        .await;
        let result = with_seta_url(addr, cli_deploy(folder.path(), false)).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
    }
}
