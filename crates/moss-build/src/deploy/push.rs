//! One moss-hosted publish, from a sealed manifest to a live site.
//!
//! The whole of what `push_site` does once its inputs are resolved: the freeze
//! latch, the deploy through seta's deploy resource (`transfer`), the event
//! sync, and the record of what went live.
//!
//! What did NOT cross is everything *before* the body, and it is app-shaped for
//! one reason: a long-lived process with a file watcher has in-flight build work
//! to drain and a sealed manifest sitting in session state. A terminal publish
//! has neither — it builds once, synchronously, and holds the manifest it just
//! sealed. So the app's own deploy module keeps the resolution, and each binary
//! reaches this function with its own answers.

use std::path::Path;

use crate::build::manifest::SealedManifest;
use crate::deploy::progress;
use crate::deploy::landed;
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
fn deploy_status_label(result: &Result<PushResult, crate::deploy::PublishError>) -> &'static str {
    match result {
        Ok(PushResult::Success { .. }) => "success",
        Ok(PushResult::NeedsSetup) => "needs_setup",
        Err(_) => "failed",
    }
}

/// Type-gated entry point for deploy work, and the one all internal callers
/// (CLI, tests, plugins, future parallel deploy paths) should use. The
/// `&SealedManifest` parameter is the deploy contract: the signature makes it
/// impossible to call deploy before all artifacts are registered. The `push_site` Tauri command
/// resolves the manifest from `AppState` and forwards here.
///
/// Brackets the deploy with `=== DEPLOY START/END … status=… ===` boundary
/// markers so the (now much smaller) failure span is grep-locatable by
/// `DEPLOY END … status=failed`; `deploy_id` correlates the two lines. A
/// `NeedsSetup` short-circuits earlier in `push_site_command_body`, so reaching
/// here is always a genuine deploy attempt.
pub async fn push_site_inner(sealed: &SealedManifest, cx: &PushContext<'_>) -> Result<PushResult, crate::deploy::PublishError> {
    push_site_accepting(sealed, cx, false).await
}

/// [`push_site_inner`], publishing even a removal of most of the site when
/// `accept_removals` (`--accept-removals`) says so.
async fn push_site_accepting(sealed: &SealedManifest, cx: &PushContext<'_>, accept_removals: bool) -> Result<PushResult, crate::deploy::PublishError> {
    super::profile::record(cx.folder_path, "seta", false, async {
    super::refuse_before_upload(cx.folder_path, sealed)?;
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
        push_site_inner_impl(sealed, cx, accept_removals),
        crate::infra::liveness::STALL_TIMEOUT,
        &format!("deploy {deploy_id}"),
    )
    .await;
    log::info!(
        "=== DEPLOY END {deploy_id} status={} ===",
        deploy_status_label(&result)
    );
    result
    }).await
}

/// The bytes are live, so nothing uploads, but a landing's local half can still
/// be owed: naming a channel changes no output, so this is the only place that
/// copy gets planned. The app waits for a verdict either way, checked against
/// what the server said is live when it answered.
async fn settle_already_live(
    sealed: &SealedManifest,
    cx: &PushContext<'_>,
    environment: crate::config::environment::HostingEnvironment,
    server_says: Option<&str>,
) -> PushResult {
    let PushContext { folder_path, identity, site_id, ports, .. } = *cx;
    let folder = folder_path.to_string_lossy().to_string();
    let target = crate::config::deployment::slot_for("moss", Some(site_id));
    let summary = match target {
        Some(target) if sources_moved(folder_path, sealed, &target) => {
            let history = crate::deploy::history::HistoryStore::in_vault(folder_path);
            landed::record_landed(folder_path, sealed, &target, ports, &history).await
        }
        _ => crate::deploy::change_record::PageChangeSummary::default(),
    };
    ports
        .begin_moss_verification(identity, environment.clone(), site_id, &folder, server_says.unwrap_or(sealed.generation_id()), &summary)
        .await;
    PushResult::Success { url: view_site_url(&folder, site_id, environment), files_uploaded: 0, files_removed: 0 }
}

/// Whether publishing would change a page against what this target last recorded. With no
/// record the answer is no: a landing there would read every post as new and
/// post the back catalogue.
fn sources_moved(folder: &Path, sealed: &SealedManifest, target: &str) -> bool {
    crate::build::manifest::published_record::load_for(&MossPaths::new(folder), Some(target))
        .is_some_and(|record| !crate::build::manifest::change_set::classify(Some(&record), sealed.site_hashes_view()).pages.is_empty())
}

/// Implementation of the deploy work. **Do not call directly** — call
/// [`push_site_inner`], which brackets this with the `=== DEPLOY START/END ===`
/// boundary markers; calling `_impl` directly bypasses them. The
/// `&SealedManifest` contract (deploy cannot run before artifacts are
/// registered) is documented on the wrapper.
async fn push_site_inner_impl(
    sealed: &SealedManifest,
    cx: &PushContext<'_>,
    accept_removals: bool,
) -> Result<PushResult, crate::deploy::PublishError> {
    let PushContext { folder_path, identity, site_id, sink, ports, events_lock } = *cx;
    let folder_path_str = folder_path.to_string_lossy().to_string();

    // Refuse before touching the network, through the door guard every
    // publish route shares (`site_config::publish_door`).
    crate::build::site_config::publish_door(folder_path, sealed.restricts_pages())?;

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
    let _pin = crate::build::ports::deploy::pin_generation(ports, folder_path, sealed.generation_id());

    let manifest_phase = crate::build::phase::PhaseTrace::start("manifest");
    let mut manifest = sealed.files().into_owned();
    super::profile::count("manifest_files", manifest.len() as u64);
    drop(manifest_phase);

    let site_id = site_id.to_string();
    sink.stage(progress::DeployStage::Syncing, 0, 0, "Comparing with server...");
    let client = crate::deploy::publish_client(cx.folder_path, identity, &environment).await;
    let site_dir = MossPaths::new(folder_path).generation_dir(sealed.generation_id());
    let transfer = super::transfer::Transfer {
        client: &client,
        site_id: &site_id,
        dir: &site_dir,
        sink,
        allow_removal: super::mass_remove::removal_accepted(accept_removals),
    };

    // A server that cannot serve members-only files to members alone refuses
    // them, and is sent the public files: a generation of their own, named by
    // their own hash, so no id ever stands for two sets of files.
    let mut opened = transfer.open(&manifest).await;
    if crate::build::emit::members::has_gated(&manifest) && matches!(opened, Err(crate::seta::client::SetaError::GatedNotEnforceable { .. })) {
        manifest = crate::build::emit::members::public_files(&manifest);
        opened = transfer.open(&manifest).await;
    }
    let opened = opened.map_err(super::profile::profiled_refusal)?;
    // Live with no commit counts: the server was already serving this deploy.
    if opened.state == crate::seta::deploys::DeployState::Live && opened.files_updated.is_none() {
        log::info!("deploy: generation {} already live on server, skipping deploy", opened.deploy);
        return Ok(settle_already_live(sealed, cx, environment, opened.live_generation.as_deref()).await);
    }
    let total = opened.needed.len() as u32;

    log::info!("deploy: shipping generation {} ({} files, {} needed)", opened.deploy, manifest.len(), total);
    let commit_result = transfer.finish(&mut manifest, opened).await?;
    let generation = commit_result.deploy.clone();
    let _after_publish = crate::build::phase::PhaseTrace::start("after_publish");

    // Record what just went live, so the NEXT publish can say what it will
    // change instead of showing a flat file count.
    //
    // Placement is the whole correctness argument. This sits after
    // the deploy went live and after the skew check, so no record is
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
            &generation,
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

    // 10b. Bring the site's records home: subscribers, form entries,
    //     comments, bookings, analytics. Non-fatal: the site is already live.
    //     The events lock keeps this from racing the panel's own analytics
    //     pull in the same process; the folder lock covers other processes.
    {
        // Each part's paging loop is unbounded and bumps liveness per page,
        // so silence here is a genuine hang and the stall watchdog judges it.
        let _guard = events_lock.lock().await;
        let sources = crate::vault::read_back::Sources::new(&client, &environment, &site_id);
        for (part, pulled) in crate::vault::read_back::pull(&sources, folder_path, &crate::vault::read_back::Part::ALL).await {
            if let Err(e) = pulled {
                log::warn!(target: "deploy", "{} read-back after publish failed: {}", part.name(), e);
            }
        }
    }

    // 10c. What the next build's rename detection diffs against used to be a
    // second file taken here, and only here — so a site published through a
    // deploy plugin never got one. It is now a field on the publish record that
    // `landed::record_what_is_live` fills, which both publish paths reach: on
    // this one from `record_landed` above, still after the deploy went live and over
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

    // The address hosting serves the site at, as it answered: which of the
    // environment's suffixes a site is under is the server's to say, and the
    // next build bakes what is stored here.
    //
    // This is the SUBDOMAIN url on purpose: `last_deployment_url` must stay
    // the original serving URL even when a custom domain exists (the publish
    // panel's "Also redirects from …" caption derives from it — see
    // `build_deployment_status`). The user-facing success URL is resolved
    // separately below, after the domain orchestrator has run.
    let hosted = crate::deploy::hosted_address(&environment, &site_id, commit_result.url.as_deref().unwrap_or_default());
    let subdomain_url = hosted.clone().unwrap_or_else(|| environment.assumed_site_url(&site_id));
    let deployed_at = commit_result.timestamp.and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    if let Err(e) = crate::vault::deployment_state::record_publish(
        &folder_path_str,
        crate::vault::deployment_state::PublishOutcome {
            url: subdomain_url,
            deployed_at,
            dns_target: moss_dns_target,
            generation_id: Some(generation.clone()),
            site_url: hosted,
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
        files_uploaded: total,
        files_removed: commit_result.files_removed.unwrap_or(0),
    })
}


/// The refusals a terminal publish can be told to go past. Named fields,
/// because adjacent booleans compile silently when swapped and each means
/// something different for a live site.
#[derive(Debug, Clone, Copy, Default)]
pub struct PublishOverrides {
    /// `--overwrite-newer`: publish over a live site another copy published
    /// after this folder's last publish.
    pub overwrite_newer: bool,
    /// `--accept-removals`: accept losing exactly the addresses the build
    /// found going offline unasked.
    pub accept_removals: bool,
    /// `--accept-stale-content`: accept exactly the pages this build found
    /// rolling back to older source content.
    pub accept_stale_content: bool,
}

impl PublishOverrides {
    /// Called between the build and the gates that read its verdicts. Removals
    /// are accepted earlier, inside the build's seal, so the build never
    /// announces an address it was told to let go.
    pub(crate) fn accept(&self, folder: &Path, sealed: Option<&SealedManifest>) {
        let records = crate::system::build_records::records();
        let key = folder.to_string_lossy();
        if let (true, Some(sealed)) = (self.accept_stale_content, sealed) {
            records.accept_stale_content(&key, super::removal_gate::stale_content(folder, sealed));
        }
    }
}

/// One moss-hosted publish, from a folder to a result — the build included.
///
/// The twin of [`super::prebuilt::run_prebuilt_deploy`] for the route that
/// needs moss's own build, and the whole of what `moss deploy <folder>` does in
/// a process with no window. The app does not call this: it publishes from a
/// manifest a watcher already sealed, which is the resolution
/// the app's own deploy module keeps.
///
/// The build is synchronous and one-shot (`exits_after_build`), so the seal
/// happens inline and [`super::one_shot::build_and_seal`] has the manifest by the time
/// it returns. Both seal arms use the host's own announcer, and
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
/// count the app publishes through would diverge from it. `--strict` belongs to `build`, where the caller decides.
///
/// One refusal is the terminal's alone, on purpose: a folder behind the live
/// site, lifted by `overwrite_newer` — see [`crate::deploy::resolve_publish_inputs`].
pub async fn run_hosted_deploy(
    folder: &Path,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
    requested_site_id: Option<&str>,
    overrides: PublishOverrides,
    sink: &std::sync::Arc<dyn progress::DeploySink>,
) -> Result<PushResult, crate::deploy::PublishError> {
    super::profile::record(folder, "seta", false, async {
    let folder_str = folder.to_string_lossy().to_string();

    // Before the build, not after: a folder with no site cannot publish however
    // well it builds, and an evicted identity key is knowable at t=0 — asking
    // for it is usually what makes it arrive.
    let Some((site_id, identity)) =
        crate::deploy::resolve_publish_inputs(folder, requested_site_id, overrides.overwrite_newer, sink).await?
    else {
        return Ok(PushResult::NeedsSetup);
    };

    // The stage-write lock every concurrent-build path takes. `run_pipeline`
    // reaches for it, and so does the seal tail.
    let _session = crate::system::folder_session::register_session(&folder_str);

    sink.stage(progress::DeployStage::Preparing, 0, 0, "Building the site...");

    let root = crate::vault::paths::VaultRoot::resolve(folder);
    let host = host_ports(&folder_str);
    let taken = super::one_shot::build_and_seal(&root, host, plugins, overrides.accept_removals).await?;

    // Only after the build: the verdict this reads is the build's own, and
    // asking before it would refuse on the previous run's answer or on none.
    overrides.accept(folder, taken.as_ref());
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
    push_site_accepting(&sealed, &cx, overrides.accept_removals).await
    }).await
}

#[cfg(test)]
#[path = "push_deploy_resource_tests.rs"]
pub(crate) mod deploy_resource_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use super::deploy_resource_tests::{DeployStandIn, Seta};
    use crate::deploy::progress::DeploySink;

    /// The upload phase's byte credit is a liveness bump. If this stops being
    /// true, a publish that is uploading steadily for longer than STALL_TIMEOUT
    /// gets cancelled while healthy — the 2026-08-04 failure, reintroduced.
    #[test]
    fn upload_byte_credit_is_a_liveness_bump() {
        let st = progress::UploadProgressState::new(4096, 1, 0);
        let before = crate::infra::liveness::thread_bump_count();
        st.credit_bytes(1024);
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
        let err: Result<PushResult, crate::deploy::PublishError> = Err("boom".to_string().into());
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
    /// connection-refused, which every caller past the deploy going live in
    /// `push_site_inner_impl` already treats as best-effort (e.g.
    /// `vault::analytics::sync_events`'s non-fatal warn). `Connection: close`
    /// on every canned response forces a fresh TCP connection per request
    /// rather than a pooled one this single-shot listener cannot serve.
    async fn mock_seta_sequence(responses: Vec<&'static [u8]>) -> std::net::SocketAddr {
        let (addr, _last_request) = mock_seta_sequence_capturing(responses).await;
        addr
    }

    /// Same as [`mock_seta_sequence`], but also hands back the LAST call's
    /// raw request bytes.
    async fn mock_seta_sequence_capturing(
        responses: Vec<&'static [u8]>,
    ) -> (std::net::SocketAddr, tokio::sync::oneshot::Receiver<Vec<u8>>) {
        use tokio::net::TcpListener;
        let capture = responses.len().saturating_sub(1);

        let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let mut tx = Some(tx);
            for (i, resp) in responses.into_iter().enumerate() {
                let (mut stream, raw) = crate::test_accept_skipping_whoami(&listener).await;
                crate::test_respond_and_close(&mut stream, resp).await;
                if i == capture {
                    if let Some(tx) = tx.take() {
                        let _ = tx.send(raw);
                    }
                }
            }
        });

        (addr, rx)
    }

    /// One page, `index.html` — enough for `push_site_inner_impl` to reach
    /// the open, whose scripted answer asks for nothing, so no bytes for it
    /// are ever read off disk.
    fn sealed_fixture(folder: &std::path::Path) -> SealedManifest {
        pending_fixture(folder).seal()
    }

    fn pending_fixture(folder: &std::path::Path) -> PendingManifest {
        let mut pending = PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(folder));
        let sp = ServedPath::from_source("index.html").unwrap();
        pending.register(&sp, b"<html>Home</html>", HashBucket::Files);
        pending
    }

    #[tokio::test]
    async fn old_clean_seal_cannot_publish_over_a_latest_pending_attempt() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let previous_url = std::env::var("MOSS_SETA_URL").ok();
        std::env::set_var("MOSS_SETA_URL", "http://127.0.0.1:0");

        let dir = tempfile::tempdir().unwrap();
        let sealed = sealed_fixture(dir.path());
        crate::system::build_records::records().install_publish_preflight(
            dir.path().to_str().unwrap(),
            crate::build::types::PublishPreflightProjection {
                build_generation: 2,
                missing_references: Vec::new(),
                unresolved_inputs: vec!["about.md".into()],
                drafts_linked: vec![],
                remote_pictures: vec![],
            },
        );
        let identity = Identity::generate().unwrap();
        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(), identity: &identity, site_id: "pending-test",
            sink: &sink, ports: &spy, events_lock: &events_lock,
        };
        let result = push_site_inner(&sealed, &cx).await;
        match previous_url {
            Some(url) => std::env::set_var("MOSS_SETA_URL", url),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }
        let error = result.expect_err("latest pending attempt must refuse before network");
        assert!(error.to_string().contains("about.md"), "{error}");
        assert!(spy.events.lock().unwrap().is_empty());
    }

    /// One recorded `begin_moss_verification` call.
    struct VerificationCall {
        site_id: String,
        folder_path: String,
        generation_id: String,
        summary: crate::deploy::change_record::PageChangeSummary,
    }

    /// One event `SpyPorts` observed, in the order it happened — the shape
    /// `a_landed_publish_calls_begin_moss_verification` needs to assert
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

    /// A canned JSON reply; the mock wants `'static` bytes.
    fn json_reply(body: String) -> &'static [u8] {
        let reply = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        Box::leak(reply.into_bytes().into_boxed_slice())
    }

    /// Open's answer when the site already serves `deploy`: nothing committed.
    fn already_live(deploy: &str, live_generation: &str) -> &'static [u8] {
        json_reply(serde_json::json!({ "deploy": deploy, "state": "live", "needed": [], "live_generation": live_generation }).to_string())
    }

    /// Open's answer when it committed `deploy` itself, having nothing to ask for.
    fn committed_by_open(deploy: &str, url: &str, files_removed: u32) -> &'static [u8] {
        json_reply(serde_json::json!({ "deploy": deploy, "state": "live", "needed": [], "url": url, "files_updated": 0,
            "files_removed": files_removed, "timestamp": 1_700_000_000, "live_generation": deploy }).to_string())
    }

    /// Push `sealed` against a server giving `responses`, what the ports saw.
    async fn push_against(vault: &std::path::Path, sealed: &SealedManifest, responses: Vec<&'static [u8]>) -> Vec<SpyEvent> {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();
        let addr = mock_seta_sequence(responses).await;
        std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
        let identity = Identity::generate().expect("generate identity");
        std::fs::create_dir_all(MossPaths::new(vault).generation_dir(sealed.generation_id())).unwrap();
        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext { folder_path: vault, identity: &identity, site_id: "site-1", sink: &sink, ports: &spy, events_lock: &events_lock };
        let result = push_site_inner(sealed, &cx).await;
        match prev_url {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }
        assert!(result.is_ok(), "{result:?}");
        let events = std::mem::take(&mut *spy.events.lock().unwrap());
        events
    }

    fn verified_generation(events: &[SpyEvent]) -> Option<String> {
        events.iter().find_map(|e| match e {
            SpyEvent::Verification(call) => Some(call.generation_id.clone()),
            _ => None,
        })
    }

    /// Naming a channel on a live post changes no output, so the server is
    /// already on this generation. The copy is planned anyway, and the publish
    /// still gets a verdict.
    #[tokio::test]
    async fn a_generation_already_live_plans_the_copy_a_post_now_names() {
        use crate::deploy::landed::tests::{land, seal_post_naming, syndicating_vault};
        let vault = syndicating_vault();
        land(vault.path(), &seal_post_naming(vault.path(), None)).await;
        let sealed = seal_post_naming(vault.path(), Some(&["notes"]));

        let live = already_live(sealed.generation_id(), sealed.generation_id());
        let events = push_against(vault.path(), &sealed, vec![live]).await;

        let records = crate::syndication::record::Records::in_vault(&MossPaths::new(vault.path()));
        assert!(records.exists("aaaa0001", "notes"), "the copy the post now names is planned");
        assert_eq!(verified_generation(&events).as_deref(), Some(sealed.generation_id()));
    }

    /// A fresh clone of a live site has no record to compare with. Planning
    /// there would make every post with a channel due: the whole back catalogue.
    #[tokio::test]
    async fn a_generation_already_live_with_no_record_plans_nothing() {
        use crate::deploy::landed::tests::{seal_post_naming, syndicating_vault};
        let vault = syndicating_vault();
        let sealed = seal_post_naming(vault.path(), Some(&["notes"]));

        let live = already_live(sealed.generation_id(), sealed.generation_id());
        let events = push_against(vault.path(), &sealed, vec![live]).await;

        let records = crate::syndication::record::Records::in_vault(&MossPaths::new(vault.path()));
        assert!(!records.exists("aaaa0001", "notes"));
        assert_eq!(verified_generation(&events).as_deref(), Some(sealed.generation_id()));
    }

    /// With nothing to commit the server's own answer is what is live, and it
    /// can differ from the sealed id; verifying the sealed id would report a
    /// good publish as serving an older generation.
    #[tokio::test]
    async fn nothing_to_commit_verifies_the_generation_the_server_named() {
        let dir = tempfile::tempdir().unwrap();
        let sealed = sealed_fixture(dir.path());
        let live = already_live(sealed.generation_id(), "server-side-id");

        let events = push_against(dir.path(), &sealed, vec![live]).await;

        assert_eq!(verified_generation(&events).as_deref(), Some("server-side-id"));
    }

    /// The row's own test: `push_site_inner` must call
    /// `ports.begin_moss_verification(...)` once the deploy is live AND `record_landed` (`after_landing`) has already run —
    /// task 4-6 moved the burst past landing so it can carry the same
    /// `PageChangeSummary` landing computed, never a second copy — carrying
    /// this publish's generation id and that summary. Ablation lives at the
    /// call site in `push_site_inner_impl` — comment it out and this
    /// assertion goes red with zero recorded calls.
    #[tokio::test]
    async fn a_landed_publish_calls_begin_moss_verification() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();

        let identity = Identity::generate().expect("generate identity");
        let dir = tempfile::tempdir().unwrap();
        let sealed = sealed_fixture(dir.path());
        let addr = mock_seta_sequence(vec![committed_by_open(sealed.generation_id(), "https://verify-test.mosspub.com", 1)]).await;
        std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
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
        let dir = tempfile::tempdir().unwrap();
        let sealed = sealed_fixture(dir.path());
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

        let err = result.expect_err("publish must be refused, not attempted").to_string();
        assert!(err.contains("schema_version") && err.contains("newer"), "got: {err}");
        assert!(
            spy.events.lock().unwrap().is_empty(),
            "no deploy port should have been touched — the refusal must come before any network activity"
        );
    }

    /// A generation that restricted a page publishes only once the site names
    /// the moss its members pages need; a config that cannot take the floor
    /// refuses the publish before any network call.
    #[tokio::test]
    async fn a_members_generation_refuses_when_the_floor_cannot_be_written() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let prev_url = std::env::var("MOSS_SETA_URL").ok();
        let closed_addr = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        std::env::set_var("MOSS_SETA_URL", format!("http://{closed_addr}"));

        let identity = Identity::generate().expect("generate identity");
        let dir = tempfile::tempdir().unwrap();
        let mut pending = pending_fixture(dir.path());
        pending.mark_restricts_pages();
        let sealed = pending.seal();
        std::fs::create_dir_all(MossPaths::new(dir.path()).generation_dir(sealed.generation_id())).unwrap();
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        std::fs::write(dir.path().join(".moss/config.toml"), "[site\n").unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(), identity: &identity, site_id: "members-floor-test",
            sink: &sink, ports: &spy, events_lock: &events_lock,
        };
        let result = push_site_inner(&sealed, &cx).await;
        match prev_url {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }

        let err = result.expect_err("publish must be refused, not attempted").to_string();
        assert!(err.contains("could not write .moss/config.toml"), "got: {err}");
        assert!(spy.events.lock().unwrap().is_empty());
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
        crate::identity::Identity::generate().unwrap().save(tmp.path()).unwrap();
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
        cli_deploy_accepting(folder, PublishOverrides { overwrite_newer, ..Default::default() }).await
    }

    async fn cli_deploy_accepting(folder: &Path, overrides: PublishOverrides) -> Result<PushResult, String> {
        run_hosted_deploy(
            folder,
            &crate::cli::host::cli_host_ports,
            crate::build::PluginMode::Skip,
            None,
            overrides,
            &progress::silent(),
        )
        .await
        .map_err(|e| e.to_string())
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
        assert!(err.to_string().contains("published from another copy"), "got: {err}");
        let probe = String::from_utf8_lossy(&probe.await.expect("the probe is the one request made")).to_string();
        assert!(probe.starts_with("GET /api/sites/behind-test/generation"), "got: {probe}");
    }

    /// A folder whose last publish served its page `kept.md` at `/gone/`,
    /// where this build serves nothing: the page's old address would go
    /// offline with nobody having asked.
    fn folder_that_lost_a_page() -> tempfile::TempDir {
        let tmp = folder_that_last_published(false);
        std::fs::write(tmp.path().join("kept.md"), "---\ntitle: Kept\n---\n\nStill here.\n").unwrap();
        crate::build::manifest::published_record::save(
            &MossPaths::new(tmp.path()),
            &crate::build::manifest::change_set::PublishedSnapshot {
                generation_id: "ours".to_string(),
                target: "moss:behind-test".to_string(),
                published_at: "2026-09-20T09:12:30+00:00".to_string(),
                files: [("gone/index.html".to_string(), "100644:abc".to_string())].into(),
                source_to_output: [("kept.md".to_string(), "gone/index.html".to_string())].into(),
                asset_source_to_output: Some(Default::default()),
                ..Default::default()
            },
        )
        .unwrap();
        tmp
    }

    /// The rule that stops a publish taking an address offline unasked. The
    /// server answers only the copy-behind probe: the refusal comes after the
    /// build and before anything is synced or uploaded. Ablated by deleting
    /// the rule from `refuse_publish`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_deploy_that_would_take_an_address_offline_is_refused_with_both_ways_out() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_lost_a_page();
        let addr = mock_seta_sequence(vec![json_200(r#"{"generation_id":"ours"}"#)]).await;

        let result = with_seta_url(addr, cli_deploy(folder.path(), false)).await;

        let err = result.expect_err("an unexplained removal must stop the publish");
        assert!(err.contains("/gone/"), "{err}");
        assert!(err.contains("--accept-removals"), "{err}");
    }

    /// The whole path for a moved page: the refusal prints the `[redirects]`
    /// line, and the very line it printed, added to the config, lets the next
    /// publish through with no `--accept-removals`. Ablated by dropping the
    /// redirect emission (the line stays in the config and the refusal stands).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_moved_page_publishes_once_the_printed_redirect_is_added() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(false);
        std::fs::write(folder.path().join("moved.md"), "---\ntitle: Moved\n---\n\nNew home.\n").unwrap();
        crate::build::manifest::published_record::save(
            &MossPaths::new(folder.path()),
            &crate::build::manifest::change_set::PublishedSnapshot {
                generation_id: "ours".to_string(),
                target: "moss:behind-test".to_string(),
                published_at: "2026-09-20T09:12:30+00:00".to_string(),
                source_to_output: [("moved.md".to_string(), "old/index.html".to_string())].into(),
                files: [("old/index.html".to_string(), "100644:abc".to_string())].into(),
                asset_source_to_output: Some(Default::default()),
                ..Default::default()
            },
        )
        .unwrap();
        let addr = mock_seta_sequence(vec![
            json_200(r#"{"generation_id":"ours"}"#),
            json_200(r#"{"generation_id":"ours"}"#),
            already_live("live", "live"),
        ])
        .await;

        let refused = with_seta_url(addr, cli_deploy(folder.path(), false)).await;
        let text = refused.expect_err("the page moved and nothing answers at the old address");
        let line = text
            .split_once("add ")
            .and_then(|(_, rest)| rest.split_once(" under [redirects]"))
            .map(|(line, _)| line.to_string())
            .unwrap_or_else(|| panic!("the refusal prints the redirect line: {text}"));
        assert_eq!(line, "\"/old/\" = \"/moved/\"");

        std::fs::write(folder.path().join(".moss/config.toml"), format!("[redirects]\n{line}\n")).unwrap();
        let result = with_seta_url(addr, cli_deploy(folder.path(), false)).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
    }

    /// The whole path for a moved folder: an HTML page under it is refused as moved
    /// (not passed as deleted), and one folder line keeps every address under
    /// the old folder, so the next publish goes through.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_moved_folder_publishes_once_one_folder_redirect_is_added() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(false);
        let bytes = b"<!doctype html><title>Plan</title>";
        std::fs::create_dir_all(folder.path().join("review/client/old")).unwrap();
        std::fs::write(folder.path().join("review/client/old/plan.html"), bytes).unwrap();
        let hash = crate::types::content::file_entry(&crate::build::assets::paths::compute_binary_hash(bytes));
        crate::build::manifest::published_record::save(
            &MossPaths::new(folder.path()),
            &crate::build::manifest::change_set::PublishedSnapshot {
                generation_id: "ours".to_string(),
                target: "moss:behind-test".to_string(),
                published_at: "2026-09-20T09:12:30+00:00".to_string(),
                files: [("review/old/plan.html".to_string(), hash)].into(),
                asset_source_to_output: Some([("review/old/plan.html".to_string(), "review/old/plan.html".to_string())].into()),
                ..Default::default()
            },
        )
        .unwrap();
        let addr = mock_seta_sequence(vec![
            json_200(r#"{"generation_id":"ours"}"#),
            json_200(r#"{"generation_id":"ours"}"#),
            already_live("live", "live"),
        ])
        .await;

        let refused = with_seta_url(addr, cli_deploy(folder.path(), false)).await;
        let text = refused.expect_err("the file moved and nothing answers at its old address");
        assert!(text.contains(r#""/review/old/plan.html" = "/review/client/old/plan.html""#), "{text}");

        std::fs::write(folder.path().join(".moss/config.toml"), "[redirects]\n\"/review/old/\" = \"/review/client/old/\"\n").unwrap();
        let result = with_seta_url(addr, cli_deploy(folder.path(), false)).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
    }

    /// `--accept-removals` records the set this build found and the publish
    /// goes on to the sync, which reports nothing to change.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accepting_the_removals_publishes() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_lost_a_page();
        let addr = mock_seta_sequence(vec![
            json_200(r#"{"generation_id":"ours"}"#),
            already_live("live", "live"),
        ])
        .await;

        let (host, mut warnings) = crate::ops::serve::events::collect_removal_warnings();
        let result = with_seta_url(addr, run_hosted_deploy(
            folder.path(), &host, crate::build::PluginMode::Skip, None,
            PublishOverrides { accept_removals: true, ..Default::default() }, &progress::silent(),
        )).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
        assert!(warnings().is_empty(), "accepted removals must not ask for action");
        let removed = crate::system::build_records::records()
            .removed_addresses(&folder.path().to_string_lossy()).unwrap();
        assert!(removed.iter().any(|r| r.path == "gone/index.html"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)] // The server URL is process-global.
    async fn stale_content_requires_acceptance_on_the_hosted_route() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let history = folder.path().join(".moss/history/publishes");
        std::fs::create_dir_all(&history).unwrap();
        std::fs::write(history.join("2026-01-01.json"), serde_json::json!({
            "version": 1, "published_at": "2026-01-01T00:00:00Z", "target": "moss:behind-test",
            "generation_id": "previous", "entries": {
                "index.md": {"mode": "100644", "hash": "previous", "source_mtime": [4000000000u64, 0]}
            }
        }).to_string()).unwrap();
        let addr = mock_seta_sequence(vec![
            json_200(r#"{"generation_id":"ours"}"#),
            json_200(r#"{"generation_id":"ours"}"#),
            already_live("live", "live"),
        ]).await;
        let error = with_seta_url(addr, cli_deploy(folder.path(), false)).await.unwrap_err();
        assert!(error.contains("index.md") && error.contains("--accept-stale-content"), "{error}");
        let result = with_seta_url(addr, cli_deploy_accepting(folder.path(), PublishOverrides {
            accept_stale_content: true, ..Default::default()
        })).await;
        assert!(matches!(result, Ok(PushResult::Success { .. })), "{result:?}");
    }

    /// `--overwrite-newer` publishes over it: the build runs and the push
    /// reaches the sync, which reports nothing to change.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_override_publishes_over_a_newer_live_site() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let addr = mock_seta_sequence(vec![
            already_live("theirs", "theirs"),
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
        let sealed = sealed_fixture(folder.path());
        std::fs::create_dir_all(MossPaths::new(folder.path()).generation_dir(sealed.generation_id())).unwrap();
        let addr = mock_seta_sequence(vec![
            already_live("theirs", "theirs"),
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
            crate::deploy::prebuilt::run_prebuilt_deploy(folder.path(), site.path(), None, false, false, &progress::silent()),
        )
        .await;

        let err = result.expect_err("a copy behind the live site must not publish");
        assert!(err.to_string().contains("published from another copy"), "got: {err}");
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

        // The guard finds this copy's own generation live, so it goes ahead.
        let seta = DeployStandIn::serve(Seta { generation: Some(serde_json::json!({ "generation_id": "ours", "deployed_at": 1790000000 })), ..Seta::default() }, Box::new(|_, _| None)).await;
        let addr = seta.addr;
        let published = with_seta_url(
            addr,
            crate::deploy::prebuilt::run_prebuilt_deploy(folder.path(), site.path(), None, false, false, &progress::silent()),
        )
        .await;
        assert!(matches!(published, Ok(PushResult::Success { files_uploaded: 1, .. })), "got: {published:?}");

        let addr = mock_seta_sequence(vec![
            // The guard: the prebuilt tree is live.
            json_200(live_prebuilt),
            already_live(&prebuilt_gen, &prebuilt_gen),
        ])
        .await;
        let result = with_seta_url(addr, cli_deploy(folder.path(), false)).await;

        assert!(matches!(result, Ok(PushResult::Success { .. })), "got: {result:?}");
    }

    /// The hosting server's commit response counts every file in the site; the
    /// publish result must report the files this publish sent instead. Here the
    /// comparison needs one file and the server answers 1039. Ablated by
    /// reading `commit_result.files_updated` back into `files_uploaded` in
    /// `push_prebuilt_inner`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_prebuilt_publish_reports_the_files_it_sent_not_the_site_total() {
        let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let folder = folder_that_last_published(true);
        let site = prebuilt_site();
        let seta = DeployStandIn::serve(Seta { generation: Some(serde_json::json!({ "generation_id": "ours", "deployed_at": 1790000000 })), committed: serde_json::json!({ "files_updated": 1039 }), ..Seta::default() }, Box::new(|_, _| None)).await;
        let addr = seta.addr;
        let published = with_seta_url(
            addr,
            crate::deploy::prebuilt::run_prebuilt_deploy(folder.path(), site.path(), None, false, false, &progress::silent()),
        )
        .await;
        assert!(matches!(published, Ok(PushResult::Success { files_uploaded: 1, .. })), "got: {published:?}");
        assert_profile_upload_count(folder.path());
    }

    /// The same, through moss's own build route (`push_site_inner`). Ablated
    /// the same way at that function's return.
    #[tokio::test]
    async fn a_hosted_publish_reports_the_files_it_sent_not_the_site_total() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let seta = DeployStandIn::serve(Seta { generation: Some(serde_json::json!({ "generation_id": "ours", "deployed_at": 1790000000 })), committed: serde_json::json!({ "files_updated": 1039 }), ..Seta::default() }, Box::new(|_, _| None)).await;
        let addr = seta.addr;

        let dir = tempfile::tempdir().unwrap();
        let mut pending = PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(dir.path()));
        let sp = ServedPath::from_source("index.html").unwrap();
        pending.register_hashed(&sp, &crate::types::content::file_entry("0000000000000000"), HashBucket::Files);
        let sealed = pending.seal();

        let identity = Identity::generate().expect("generate identity");
        let gen_dir = MossPaths::new(dir.path()).generation_dir(sealed.generation_id());
        std::fs::create_dir_all(&gen_dir).unwrap();
        std::fs::write(gen_dir.join("index.html"), b"<html>x</html>").unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(),
            identity: &identity,
            site_id: "count-test",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };
        let result = with_seta_url(addr, push_site_inner(&sealed, &cx)).await;
        assert!(matches!(result, Ok(PushResult::Success { files_uploaded: 1, .. })), "got: {result:?}");
        assert_profile_upload_count(dir.path());
    }

    /// One hosted publish of a one-file site, its commit answered with the
    /// address `answered`. Returns the folder and the address the publish
    /// reported; the caller has pinned the environment.
    async fn hosted_publish_answered_with(answered: &str) -> (tempfile::TempDir, String) {
        let seta = DeployStandIn::serve(Seta { generation: Some(serde_json::json!({ "generation_id": "ours", "deployed_at": 1790000000 })), committed: serde_json::json!({ "url": answered }), ..Seta::default() }, Box::new(|_, _| None)).await;
        let addr = seta.addr;

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"her-blog\"\n").unwrap();
        let mut pending = PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(dir.path()));
        let sp = ServedPath::from_source("index.html").unwrap();
        pending.register_hashed(&sp, &crate::types::content::file_entry("0000000000000000"), HashBucket::Files);
        let sealed = pending.seal();

        let identity = Identity::generate().expect("generate identity");
        let gen_dir = MossPaths::new(dir.path()).generation_dir(sealed.generation_id());
        std::fs::create_dir_all(&gen_dir).unwrap();
        std::fs::write(gen_dir.join("index.html"), b"<html>x</html>").unwrap();

        let sink = progress::silent();
        let spy = SpyPorts::default();
        let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let cx = PushContext {
            folder_path: dir.path(),
            identity: &identity,
            site_id: "her-blog",
            sink: &sink,
            ports: &spy,
            events_lock: &events_lock,
        };
        let result = with_seta_url(addr, push_site_inner(&sealed, &cx)).await;
        let Ok(PushResult::Success { url, .. }) = result else { panic!("got: {result:?}") };
        (dir, url)
    }

    /// What a folder's record and its next build say the site's address is.
    fn recorded_and_built_address(folder: &Path) -> (Option<String>, Option<String>, String) {
        let folder = folder.to_str().unwrap();
        let record = crate::build::site_config::get_domain_config(folder).unwrap();
        let built = crate::build::site_url::resolve_for_project(folder, None).unwrap();
        (record.site_url, record.last_deployment_url, built.as_str().to_string())
    }

    /// Hosting says which of its domains a site is served under, and the next
    /// build bakes that address without asking again. Ablated by deleting
    /// `site_url: hosted` from the record `push_site_inner` writes.
    #[tokio::test]
    async fn a_hosted_publish_stores_the_address_hosting_answered_and_the_next_build_bakes_it() {
        let _env = crate::PinnedEnvironment::pin(HostingEnvironment::Production);
        let (dir, reported) = hosted_publish_answered_with("https://her-blog.mosspub.com").await;
        let page = "https://her-blog.mosspub.com".to_string();
        assert_eq!(reported, page);
        assert_eq!(recorded_and_built_address(dir.path()), (Some(page.clone()), Some(page.clone()), page));
    }

    /// The answer is another machine's word. One that is not this site's
    /// address on this environment's hosting is not stored, and the folder
    /// keeps the address it was assumed to be at.
    #[tokio::test]
    async fn a_hosted_publish_does_not_store_an_address_that_is_not_this_sites() {
        let _env = crate::PinnedEnvironment::pin(HostingEnvironment::Production);
        for answered in [
            "https://evil.example",
            "https://other-site.mosspub.com",
            "https://her-blog-test.mosspub.page",
            "https://her-blog.mosspub.com.evil.example",
            "http://her-blog.mosspub.com",
        ] {
            let (dir, reported) = hosted_publish_answered_with(answered).await;
            let assumed = "https://her-blog.mosspub.page".to_string();
            assert_eq!(reported, assumed, "{answered}");
            assert_eq!(recorded_and_built_address(dir.path()), (None, Some(assumed.clone()), assumed), "{answered}");
        }
    }

    /// The same through `--prebuilt`. Ablated by deleting `site_url: hosted`
    /// from the record `push_prebuilt_inner` writes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_prebuilt_publish_stores_the_address_hosting_answered() {
        let _latch = crate::deploy::freeze::single_flight_tests::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = crate::PinnedEnvironment::pin(HostingEnvironment::Production);
        for (answered, stored, serving) in [
            ("https://behind-test.mosspub.com", Some("https://behind-test.mosspub.com"), "https://behind-test.mosspub.com"),
            ("https://evil.example", None, "https://behind-test.mosspub.page"),
        ] {
            let folder = folder_that_last_published(true);
            let site = prebuilt_site();
            let seta = DeployStandIn::serve(Seta { generation: Some(serde_json::json!({ "generation_id": "ours", "deployed_at": 1790000000 })), committed: serde_json::json!({ "url": answered }), ..Seta::default() }, Box::new(|_, _| None)).await;
            let addr = seta.addr;
            let published = with_seta_url(
                addr,
                crate::deploy::prebuilt::run_prebuilt_deploy(folder.path(), site.path(), None, false, false, &progress::silent()),
            )
            .await;
            let Ok(PushResult::Success { url, .. }) = published else { panic!("got: {published:?}") };
            assert_eq!(url, serving, "{answered}");
            let (stored_url, last_url, _) = recorded_and_built_address(folder.path());
            assert_eq!((stored_url.as_deref(), last_url.as_deref()), (stored, Some(serving)), "{answered}");
        }
    }

    fn assert_profile_upload_count(folder: &Path) {
        let text = std::fs::read_to_string(MossPaths::new(folder).profile()).unwrap();
        let records: Vec<serde_json::Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(records.len(), 1, "one profile per deploy, including nested drivers");
        assert_eq!(records[0]["kind"], "deploy");
        let counts = &records[0]["counts"];
        assert_eq!(counts["files_uploaded"], 1);
        assert_eq!(counts["files_needed"], 1);
        assert!(counts["files_needed"].as_u64().unwrap() <= counts["manifest_files"].as_u64().unwrap());
        assert_eq!(counts["server_files_updated"], 1039, "server site total stays separate");
    }

    /// A folder with no site answers `NeedsSetup` before the build, asking the
    /// server nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_folder_with_no_site_needs_setup_and_asks_the_server_nothing() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let seta = crate::seta::client::tests::StandIn::json(404, "{}").await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.md"), "# Home\n").unwrap();
        let prev = std::env::var("MOSS_SETA_URL").ok();
        std::env::set_var("MOSS_SETA_URL", seta.url());

        let result = run_hosted_deploy(dir.path(), &crate::cli::host::cli_host_ports, crate::build::PluginMode::Skip, None, Default::default(), &progress::silent()).await;

        match prev {
            Some(u) => std::env::set_var("MOSS_SETA_URL", u),
            None => std::env::remove_var("MOSS_SETA_URL"),
        }
        assert!(matches!(result, Ok(PushResult::NeedsSetup)), "got {:?}", result.map_err(|e| e.to_string()));
        assert!(seta.requests().is_empty(), "asked {:?}", seta.requests().iter().map(|r| r.path_and_query.clone()).collect::<Vec<_>>());
    }
}
