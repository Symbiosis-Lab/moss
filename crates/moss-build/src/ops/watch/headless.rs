//! The no-shell watch host: the rebuild bodies for a headless
//! `build --serve --watch`, and the process-global content-hash stash they
//! read.
//!
//! Extracted at slice C1 of the 2026-08-28 preview-server relocation plan.
//! W1 crossed the *driver* (`ops/watch.rs`) and left the rebuild bodies
//! host-side, because on the GUI they are managed-state glue. Headless they
//! are not: with no shell there is no `RebuildState`, no publish freeze (a
//! headless process cannot deploy), no progress channel and no live-port
//! resolver — what remains is worker admission, the stage-lock probe, the
//! content-hash gate and one `run_pipeline` call, all of which already live
//! in this crate. Both headless hosts — the app binary's `moss build` arm
//! (`src-tauri/src/build_shell/watch.rs::start_file_watching_headless`, which
//! adds the app-side sweep on top) and `moss-cli` — construct through here,
//! so the CLI gaining `--watch` did not mint a second copy of these bodies.
//!
//! What a host still decides arrives as [`HeadlessWatchConfig`] values: the
//! [`HostPorts`] each rebuild runs with (both are `HostPorts::headless`; the
//! two differ only in the `HostStore` — the app binary migrates `.moss` on
//! disk, moss-cli in memory), and the rebuild's [`PluginMode`].
//!
//! Known headless gap, deliberate: the sweep — the periodic disk-vs-baseline
//! backbone (`build_shell/watch/sweep.rs`) — is app-side today (it reads the
//! app's manifest cache and emits folder-health events), so the app's
//! headless arm runs it and moss-cli's watch is watcher-only until the sweep
//! crosses.

use std::sync::Arc;

use crate::build::ports::reporter::BuildReporter;
use crate::build::watch::{baseline_for_rebuild, decide_rebuild_event};
use crate::build::{run_pipeline, BuildTrigger, HostPorts, PipelineConfig, PluginMode};
use crate::ops::serve::events::CarrierReporter;
use crate::ops::watch::{worker, EventRelay, RebuildAttempt, RebuildDispatch, WatchConfig};

/// Resolve the [`HostPorts`] a build of `folder` runs with. A factory rather
/// than a value because every watch rebuild constructs a fresh set (ports are
/// not `Clone`), exactly as the app's `host_ports(app, folder)` is called per
/// rebuild.
pub type HostPortsFactory = Arc<dyn Fn(&str) -> HostPorts + Send + Sync>;

/// Everything [`start`] needs from its host.
pub struct HeadlessWatchConfig {
    /// The watched project root.
    pub folder_path: String,
    /// See [`HostPortsFactory`].
    pub host_ports: HostPortsFactory,
    /// Plugin participation for watch rebuilds, decided by
    /// [`crate::ops::run_headless_build`] and passed through unchanged.
    pub plugins: PluginMode,
}

/// Start a headless watch: the crate-side driver ([`super::start`]) wired to
/// the no-shell rebuild bodies below. Events go to the SSE carrier's bus —
/// the `CarrierReporter`/`publish` half of "one publisher per carrier"
/// (`ops/serve/events.rs`).
///
/// Returns the shutdown sender for the caller to hold: headless has no
/// managed watcher state to claim it, and dropping it stops the watch
/// immediately.
#[must_use = "dropping the shutdown sender stops the watcher immediately"]
pub async fn start(config: HeadlessWatchConfig) -> tokio::sync::oneshot::Sender<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let emit: EventRelay = Arc::new(|event| crate::ops::serve::events::publish(&event));
    let dispatch: RebuildDispatch = {
        let folder = config.folder_path.clone();
        let host_ports = config.host_ports.clone();
        let plugins = config.plugins.clone();
        Arc::new(move |req| {
            let folder = folder.clone();
            let host_ports = host_ports.clone();
            let plugins = plugins.clone();
            Box::pin(async move {
                request_rebuild(&folder, &host_ports, plugins, req).await;
            })
        })
    };
    let attempt: RebuildAttempt = {
        let folder = config.folder_path.clone();
        let host_ports = config.host_ports.clone();
        let plugins = config.plugins.clone();
        Arc::new(move |req, handle| {
            let folder = folder.clone();
            let host_ports = host_ports.clone();
            let plugins = plugins.clone();
            Box::pin(async move {
                attempt_admitted_rebuild(&folder, &host_ports, plugins, req, &handle).await
            })
        })
    };

    super::start(WatchConfig {
        folder_path: config.folder_path.clone(),
        spawner: Arc::new(crate::build::ports::spawner::TokioSpawner),
        shutdown_rx,
        emit,
        dispatch,
        attempt,
    })
    .await;

    shutdown_tx
}

/// Enqueue a rebuild with the folder's worker — the headless
/// [`RebuildDispatch`]. No `rebuild_pending` bookkeeping and no publish-freeze
/// check: both exist for the GUI's deploy quiet-wait, and a headless process
/// cannot publish.
async fn request_rebuild(
    folder_path: &str,
    host_ports: &HostPortsFactory,
    plugins: PluginMode,
    req: worker::RebuildRequest,
) {
    if !worker::worker_enabled() {
        return inline_with_pump_gate(folder_path, host_ports, plugins, req).await;
    }
    let Some(handle) = worker::get(folder_path) else {
        // Every caller is watcher-adjacent, so a worker should exist; if one
        // doesn't, building inline loses nothing (the app's fallback rule).
        log::warn!("No rebuild worker registered for '{}' — building inline", folder_path);
        return inline_with_pump_gate(folder_path, host_ports, plugins, req).await;
    };
    // Unlike the GUI, headless has no producer that can dispatch before this
    // point: nothing constructs this `request_rebuild` closure (hence nothing
    // can reach here) until `start()` registers the worker, and `start()` runs
    // only after `run_headless_build`'s own initial `run_pipeline` returns —
    // so no trigger can fire before the folder's first build completes.
    handle.enqueue(req);
}

/// The `MOSS_REBUILD_WORKER=off` / no-worker route: run the deferred
/// content-hash gate inline (as the pre-worker pump did), then build.
async fn inline_with_pump_gate(
    folder_path: &str,
    host_ports: &HostPortsFactory,
    plugins: PluginMode,
    req: worker::RebuildRequest,
) {
    if !req.gate_admits(folder_path, "inline").await {
        return;
    }
    do_rebuild_and_notify(folder_path, host_ports, plugins, &req.rename_pairs, req.trigger, None)
        .await;
}

/// One admission attempt by the folder's worker — the headless
/// [`RebuildAttempt`]: stage-lock probe → content-hash gate → admission epoch
/// → build. The GUI arm's freeze check and `rebuild_pending`/`is_rebuilding`
/// stores have no headless counterpart (nothing deploys, nothing waits for
/// quiet), so the restores here are plain `restore`, never `restore_visibly`.
async fn attempt_admitted_rebuild(
    folder_path: &str,
    host_ports: &HostPortsFactory,
    plugins: PluginMode,
    req: worker::RebuildRequest,
    handle: &Arc<worker::WorkerHandle>,
) -> worker::AttemptOutcome {
    if !worker::worker_enabled() {
        inline_with_pump_gate(folder_path, host_ports, plugins, req).await;
        return worker::AttemptOutcome::Completed;
    }

    // STAGE-LOCK TRY-ADMISSION (phase 1b): probe — acquire-and-release — and
    // re-park when held, so a wedged holder cannot stack blocked admissions.
    // The probe is advisory; see the app arm's comment for the full rationale.
    if let Some(session) = crate::system::folder_session::registry().get(folder_path) {
        if session.try_lock_stage_write().is_none() {
            log::info!(
                target: "moss::build::watch",
                "Stage-write lock held for '{}' — re-parking the rebuild request",
                folder_path
            );
            handle.restore(req);
            return worker::AttemptOutcome::LockBusy;
        }
    }

    // ADMISSION-TIME CONTENT-HASH GATE (phase 1b): the stat+SHA-256 pass runs
    // on the blocking pool against the freshest stashed baseline; fail open.
    // Same owner as the app arm — see `RebuildRequest::gate_admits`.
    if !req.gate_admits(folder_path, "at admission").await {
        return worker::AttemptOutcome::Completed;
    }

    // Admission-time epoch: minted at dequeue so a build that wedges and
    // un-wedges after its successor can never promote stale output over it.
    let admission_epoch = crate::build::ship::next_promotion_epoch();

    let _watchdog =
        worker::OverdueWatchdog::arm(handle.clone(), folder_path, worker::REBUILD_OVERDUE_AFTER);

    let diag_rebuild_start = std::time::Instant::now();
    let built = do_rebuild_and_notify(
        folder_path,
        host_ports,
        plugins,
        &req.rename_pairs,
        req.trigger.clone(),
        Some(admission_epoch),
    )
    .await;
    log::info!(
        "[diag] preview-wait: watcher rebuild finished in {} ms",
        diag_rebuild_start.elapsed().as_millis()
    );
    if !built {
        // The slot survives failure: restore so the worker retries after
        // backoff instead of waiting for the next unrelated change event.
        handle.restore(req);
        return worker::AttemptOutcome::Failed;
    }
    worker::AttemptOutcome::Completed
}

/// Run one watch rebuild and publish its refresh event to the SSE carrier.
/// The headless half of the app's `do_rebuild_and_notify`: same baseline
/// capture, same pipeline call shape (`start_server: false` — the server is
/// already running and shares the session's serve state), same post-build
/// stash diff.
async fn do_rebuild_and_notify(
    folder_path: &str,
    host_ports: &HostPortsFactory,
    plugins: PluginMode,
    rename_pairs: &[(String, String)],
    trigger: BuildTrigger,
    admission_epoch: Option<u64>,
) -> bool {
    let source = std::path::PathBuf::from(folder_path);
    // Post-generations: the pipeline writes to .moss/build/staging/. A stable
    // path reference for the baseline diff lookup.
    let output = source.join(".moss/build/staging");

    // Baseline for the change-detection diff: prefer the PREVIOUS build's
    // race-free in-memory hashes over re-reading the asynchronously-sealed
    // hashes.json. Captured before `run_pipeline` overwrites the stash.
    let in_memory_baseline = crate::system::build_records::records().content_hashes(folder_path);
    let previous_hashes = baseline_for_rebuild(in_memory_baseline, folder_path, &output);

    let result = run_pipeline(PipelineConfig {
        root: crate::vault::paths::VaultRoot::resolve(&source),
        // Tier-1 progress goes to the carrier too, so an attached browser
        // hears the rebuild's BuildComplete — its refresh signal.
        progress: Arc::new(CarrierReporter),
        plugins,
        watch: false,         // don't restart watching from within a rebuild
        start_server: false,  // server already running
        server_port: None,
        live_port: None,
        host: (host_ports)(folder_path),
        trigger,
        // Watch rebuilds run inside the long-lived serve process.
        exits_after_build: false,
        site_url_override: None,
        admission_epoch,
    })
    .await;

    if let Err(e) = result {
        log::error!("❌ Rebuild failed: {}", e);
        return false;
    }

    // PEEK (clone, retain) the hashes THIS build stashed synchronously — the
    // slot survives as the race-free baseline for the NEXT rebuild's diff.
    let stash = crate::system::build_records::records().content_hashes(folder_path);
    let had_stash = stash.is_some();
    let rebuild_event = decide_rebuild_event(stash, folder_path, &previous_hashes, rename_pairs);

    match rebuild_event {
        None => {
            log::info!(
                "Rebuild completed with no output changes (suppressing refresh) [baseline content-stash {}]",
                if had_stash { "present" } else { "absent — diffed against on-disk hashes" }
            );
        }
        Some(event) => {
            // Once per rebuild cycle: the SSE carrier is the headless tier-2
            // bus, so the browser's preview refresh hears the change set.
            CarrierReporter.report(&crate::build::progress::PipelineEvent::FileChanged(event));
        }
    }
    true
}

