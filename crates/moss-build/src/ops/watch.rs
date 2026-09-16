//! The file-watch driver: debouncer sessions, the event pump, and the
//! per-folder rebuild worker — the long-running loop behind `--watch`.
//!
//! Crossed from `src-tauri/src/build_shell/watch.rs` at slice W1 of the
//! 2026-08-28 preview-server relocation plan (NORTH-STAR charters exactly this
//! home: "watch — loop in `ops/watch.rs` behind Spawner; the app-side task
//! stays UiBound"). The *decisions* — whether a change should rebuild, the
//! content-hash gate, rename pairing, the refresh diff — were already here in
//! [`crate::build::watch`]; this module adds the driver that runs them.
//!
//! ## The host seam, constructor-shaped
//!
//! Nothing here names a shell. The loop's three host needs arrive as
//! [`WatchConfig`] values, resolved once by whichever host starts the watch:
//!
//! * **where tasks run** — the existing [`Spawner`] port (the desktop app
//!   passes its tauri-runtime spawner; a headless build passes
//!   [`TokioSpawner`](crate::build::ports::spawner::TokioSpawner));
//! * **where events go** — an [`EventRelay`]: the app's typed Tauri bus, or
//!   the SSE carrier's bus ([`crate::ops::serve::events`]) when there is no
//!   shell. There is no `Option<AppHandle>` mode flag any more; each host has
//!   exactly one construction path (the same shape `ops/serve.rs` took at S1);
//! * **how a rebuild runs** — [`RebuildDispatch`]/[`RebuildAttempt`]: the
//!   rebuild bodies stay host-side (they are managed-state glue — baseline
//!   stashes, the publish freeze, progress channels), and the driver only
//!   decides *when* to call them.
//!
//! The worker/supervision machinery (`ops/watch/worker.rs`,
//! `ops/watch/supervision.rs`, `ops/watch/reconcile.rs`) rode along whole —
//! it was host-free already, process-global by design so the headless path
//! gets the same slots and strike rules.

use std::path::Path;
use std::sync::Arc;

use notify::Watcher;
use notify_debouncer_full::{new_debouncer, DebouncedEvent};

use crate::build::ports::spawner::Spawner;
use crate::build::watch::scope;
use crate::build::watch::{
    any_path_passes_filter, any_path_watchable, classify_trigger, collect_raw_create_keys,
    extract_rename_pairs, path_passes_filter, pump_gate, should_recompile_for_event,
    source_asset_request_paths, watcher_lost_events, PumpGate,
};
use crate::types::events::MossEvent;

pub mod headless;
pub mod reconcile;
pub mod supervision;
pub mod worker;

/// Deliver one [`MossEvent`] to whatever frontend the host has — the typed
/// Tauri bus, the SSE carrier, or both. Resolved once at construction.
pub type EventRelay = Arc<dyn Fn(MossEvent) + Send + Sync>;

/// One boxed unit future, the driver's async-callback currency.
pub type UnitFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;

/// Enqueue (or, host willing, run) one rebuild request. The host owns the
/// route — worker slot vs. the `MOSS_REBUILD_WORKER=off` inline body — and the
/// derived `rebuild_pending` bookkeeping a publish's quiet-wait reads.
pub type RebuildDispatch = Arc<dyn Fn(worker::RebuildRequest) -> UnitFuture + Send + Sync>;

/// One admission attempt by the folder's worker: dequeue → freeze / stage-lock
/// / content-hash gate → build → notify. Host-side because every step reads
/// host state; [`worker::run_worker_loop`] owns the retry policy around it.
pub type RebuildAttempt = Arc<
    dyn Fn(
            worker::RebuildRequest,
            Arc<worker::WorkerHandle>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = worker::AttemptOutcome> + Send + 'static>,
        > + Send
        + Sync,
>;

/// Everything [`start`] needs, resolved by the host before construction.
pub struct WatchConfig {
    /// The watched project root.
    pub folder_path: String,
    /// Runs the worker loop and the watcher session task.
    pub spawner: Arc<dyn Spawner>,
    /// Resolves when the host wants this folder's watch torn down. The GUI
    /// registers the sender in its managed `FileWatcherState`; a headless
    /// watch holds it for the life of the process.
    pub shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    pub emit: EventRelay,
    pub dispatch: RebuildDispatch,
    pub attempt: RebuildAttempt,
}

/// Per-event delay passed to `notify-debouncer-full`. Lowered from 500ms to
/// 250ms so a rebuild kicks off sooner after an editor save — the live preview
/// should respond quickly.
///
/// NOT a quiet period: the crate releases each raw event 250ms after that
/// event's own arrival, so it merges only what lands inside the same drain
/// (tick = 250/4 = 62.5ms). It coalesces the sub-millisecond burst one write
/// emits; it does NOT coalesce two events hundreds of milliseconds apart, and
/// raising this value would not — it would only delay both. See the
/// `build::watch` module doc and
/// `docs/archive/2026-08-31-rebuild-pairs-per-save.md`.
///
/// CROSS-LAYER CONTRACT: the frontend's `PreviewManager.REFRESH_COALESCE_MS`
/// (preview-manager.ts, currently 200ms) merges the FileChanged + BuildComplete
/// pair of ONE rebuild while keeping two DISTINCT rebuilds separate — that only
/// holds while this value stays ABOVE it (~>= 200ms). Lowering DEBOUNCE_MS below
/// that would let two genuinely-distinct rebuilds collapse into one preview
/// morph (the second edit's content would not show). Lower both together.
const DEBOUNCE_MS: u64 = 250;

/// [diag] energy: inter-arrival clock for rebuild triggers. A tight run of small
/// deltas here is the file-watcher rebuild-loop signature (cloud-sync re-emit,
/// self-trigger, save-during-copy) — the top idle-CPU suspect. Returns the gap
/// since the previous trigger (`None` on the first trigger this session).
fn rebuild_interarrival() -> Option<std::time::Duration> {
    use std::sync::{Mutex, OnceLock};
    static LAST: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();
    let cell = LAST.get_or_init(|| Mutex::new(None));
    let now = std::time::Instant::now();
    let mut guard = cell.lock().ok()?;
    guard.replace(now).map(|prev| now.duration_since(prev))
}

/// Register `folder_path`'s rebuild worker unconditionally, evicting and
/// shutting down whatever was registered before — exactly `worker::register`'s
/// own contract, plus spawning its loop. Safe to call even when a STALE
/// worker from a torn-down previous session of the same folder (a re-open)
/// might still be in the registry: `worker::register` shuts that one down and
/// replaces it, so nothing already reused it can end up enqueueing into a
/// handle that is about to stop draining its slot.
///
/// This is the primitive an OPEN sequence must call — never [`ensure_worker`],
/// whose reuse-if-present behavior is only sound for a LATER call in the same
/// sequence (see its doc). Confusing the two reintroduces exactly the race
/// part 1 below closes, on a re-open instead of a cold open.
pub fn register_worker(
    folder_path: &str,
    spawner: &Arc<dyn Spawner>,
    attempt: RebuildAttempt,
) -> Arc<worker::WorkerHandle> {
    // The folder's rebuild worker: drains the request slot one build at a
    // time, so no producer ever awaits a build inline (phase 1a of
    // docs/archive/2026-08-18-watcher-reliability-architecture.md). Spawned
    // even when the kill switch routes triggers to the inline body — the
    // publish thaw's catch-up rides the slot either way, and an idle worker
    // costs one parked task.
    let worker_handle = worker::register(folder_path);
    log::info!(
        "Rebuild path: {}",
        if worker::worker_enabled() {
            "per-folder worker (set MOSS_REBUILD_WORKER=off to revert)"
        } else {
            "inline await (MOSS_REBUILD_WORKER=off)"
        }
    );
    {
        let handle = worker_handle.clone();
        let attempt_handle = worker_handle.clone();
        drop(spawner.spawn(Box::pin(async move {
            worker::run_worker_loop(handle, move |req| attempt(req, attempt_handle.clone()))
                .await;
        })));
    }
    worker_handle
}

/// Ensure a rebuild worker is registered and running for `folder_path`,
/// reusing one that is already there instead of always minting a fresh
/// handle via [`register_worker`].
///
/// **Only sound as the SECOND call in an open sequence.** The intended use is
/// `start()` below finding the handle a host's OWN early call already
/// registered earlier in the SAME open (see `ensure_rebuild_worker` in
/// `build_shell/watch.rs`, called at folder-open, before this driver ever
/// starts) — nothing else can have touched this folder's registry entry in
/// between, so reuse is safe. It is NOT what an open sequence's own first,
/// early registration should call: an entry already present at THAT point
/// could be a stale handle from a torn-down previous session of the same
/// folder (a re-open racing its own teardown), whose loop may see
/// `shutdown_requested()` and exit without draining anything enqueued into
/// it after that point. `register_worker` is the eviction-safe primitive for
/// that first call.
///
/// Seal-persist-race-404 fix, part 1 (moss open-double-build): without a host
/// calling `register_worker` early, a trigger landing between folder-open and
/// the watcher's own `start()` found no worker at all and fell back to
/// building inline — a second, uncoordinated `run_pipeline` against the same
/// `stage_dir` the open build was still writing. See
/// `docs/archive/2026-09-15-open-double-build-race.md`.
pub fn ensure_worker(
    folder_path: &str,
    spawner: &Arc<dyn Spawner>,
    attempt: RebuildAttempt,
) -> Arc<worker::WorkerHandle> {
    match worker::get(folder_path) {
        Some(existing) => existing,
        None => register_worker(folder_path, spawner, attempt),
    }
}

/// Start file watching for live development mode.
///
/// Monitors the source folder for content file changes and triggers silent
/// recompilation for seamless preview updates. Registers the folder's rebuild
/// worker and supervision ledger, then hands the session loop to the host's
/// spawner and returns.
pub async fn start(config: WatchConfig) {
    let WatchConfig {
        folder_path,
        spawner,
        mut shutdown_rx,
        emit,
        dispatch,
        attempt,
    } = config;

    let worker_handle = ensure_worker(&folder_path, &spawner, attempt);

    // The folder's watcher-health ledger (phase 3 — supervision). The pump
    // records liveness into it, the sweep judges strikes against it, and the
    // session loop below executes its recreate verdicts.
    let health = supervision::register(&folder_path);

    drop(spawner.spawn(Box::pin(async move {
        /// Why one watcher session ended — decides whether the next begins.
        enum SessionEnd {
            /// Folder close. The only way the task exits.
            Shutdown,
            /// The sweep's strike verdict: tear down and re-create now.
            Recreate,
            /// The debouncer's thread died on its own (the channel closed).
            /// Degrade to sweep-only; the strike rule recreates on evidence.
            ChannelClosed,
        }

        // The watcher is a SESSION inside a loop, not the task's whole body
        // (phase 3): the sweep supervises it by outcomes, and its recreate
        // verdict must be executable without tearing down the worker or the
        // sweep — the correctness half of the folder. Each iteration builds a
        // fresh debouncer + subscription set; only Shutdown leaves the loop.
        'session: loop {
            // PATTERN 6 — INODE+FILE-ID PAIRING via notify-debouncer-full
            // see docs/archive/2026-05-22-editor-state-architecture.md
            // Adopted from: notify-debouncer-full (canonical),
            // Spacedrive crates/fs-watcher/src/platform/macos.rs
            //
            // The debouncer thread holds each raw notify event for 250ms (a
            // delay, not a quiet window — see DEBOUNCE_MS) and stitches
            // `Modify(Name(From))` + `Modify(Name(To))` pairs into a single
            // `Modify(Name(Both))` event with `paths = [from, to]` by matching
            // inodes/file-IDs from the file-id cache.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<
                notify_debouncer_full::DebounceEventResult,
            >();

            let mut debouncer = match new_debouncer(
                std::time::Duration::from_millis(DEBOUNCE_MS),
                None, // tick_rate = timeout/4 = 62.5ms
                move |result| {
                    let _ = tx.send(result);
                },
            ) {
                Ok(d) => d,
                Err(e) => {
                    // NOT fatal to the folder (it was until phase 3, when this
                    // tore the worker down with it): the sweep is the
                    // correctness backbone and its drift dispatches need the
                    // worker alive. Degrade to sweep-only and let the strike
                    // rule retry — the design's "folder-open handoff leaves
                    // folder unwatched" row ends "the strike rule creates the
                    // watcher".
                    log::error!(
                        "❌ Failed to create file watcher: {} — sweep-only until a retry succeeds",
                        e
                    );
                    tokio::select! {
                        _ = &mut shutdown_rx => break 'session,
                        _ = health.recreate_requested() => continue 'session,
                    }
                }
            };

            // Subscribe to the project's content — NOT to the project root (#960).
            //
            // Watching the root recursively subscribed moss to its own build
            // output. A build writes ~5700 files, which overflows the FSEvents
            // buffer, and the overflow arrives as a PATHLESS rescan event that no
            // per-event filter can attribute — so it forced a full rebuild, which
            // wrote the output again, forever. `scope::watch_targets` returns the
            // content-only set; see that module for why this cannot be a
            // `RecursiveMode` change on macOS.
            //
            // The file-id cache gets exactly the same set. It used to index the
            // root recursively, which meant it walked the whole build output
            // (~25k files on the reported site) to answer rename questions about
            // content it must never see.
            let root = Path::new(&folder_path);
            let mut watched: Vec<scope::WatchTarget> = Vec::new();
            for (path, mode) in scope::watch_targets(root) {
                if let Err(e) = debouncer.watcher().watch(&path, mode) {
                    // One unreadable subdirectory must not cost the whole watcher.
                    log::warn!("⚠️ Not watching '{}': {}", path.display(), e);
                    continue;
                }
                debouncer.cache().add_root(&path, mode);
                watched.push((path, mode));
            }
            // An empty set is survivable, not fatal: the reconciler below picks up
            // the first content that appears. Returning here would kill the watcher
            // for the session — reachable on macOS, where there is no root target,
            // for a project holding only dotfiles and a bare `.moss/`.
            if watched.is_empty() {
                log::warn!(
                    "⚠️ Nothing watchable in '{}' yet — waiting for content to appear",
                    folder_path
                );
            }

            // Success log (#572). Without this, "watcher started fine but no events"
            // and "watcher silently never started" look identical in the log.
            log::info!(
                "✅ File watcher active: {} ({} target(s))",
                folder_path,
                watched.len()
            );

            // Because the root is not watched recursively, a NEW top-level entry
            // is not covered by any existing subscription. Reconcile the set on a
            // low-frequency tick: one `read_dir` over ~10 entries, which also
            // unsubscribes entries that were removed.
            let mut reconcile = tokio::time::interval(reconcile::INTERVAL);
            reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            reconcile.tick().await; // the first tick completes immediately

            // Process debounced batches until shutdown, recreate, or channel death
            let end = loop {
                tokio::select! {
                    res = &mut shutdown_rx => {
                        // `Err` = sender dropped without sending: nobody asked it to
                        // stop. The one exit path that used to leave no trace at all.
                        if res.is_err() { log::warn!("⚠️ File watcher for '{folder_path}' stopped unrequested — its shutdown sender was dropped"); }
                        break SessionEnd::Shutdown;
                    }
                    // The sweep's strike verdict (see watch/supervision.rs):
                    // the stream missed a change and stayed silent past the
                    // aging window, so it is presumed dead — the incident's
                    // FSEvents death emits nothing that could say so itself.
                    _ = health.recreate_requested() => {
                        break SessionEnd::Recreate;
                    }
                    _ = reconcile.tick() => {
                        reconcile::targets(&mut debouncer, root, &mut watched);
                    }
                    // Process file events (one batch per debounce window)
                    event_result = rx.recv() => {
                        match event_result {
                            Some(Ok(events)) => {
                                // Liveness, judged pre-filter: any delivered
                                // batch proves the stream is alive, even one
                                // the pump goes on to suppress entirely.
                                health.note_batch();
                                handle_debounced_batch(events, &folder_path, &emit, &dispatch).await;
                            }
                            Some(Err(errors)) => {
                                for e in errors {
                                    log::error!("❌ File watcher error: {}", e);
                                }
                            }
                            None => {
                                break SessionEnd::ChannelClosed;
                            }
                        }
                    }
                }
            };

            // Stop the debouncer thread cleanly. `Debouncer` also drops itself on
            // out-of-scope, but calling stop() here makes intent explicit and
            // waits for the background thread to join.
            debouncer.stop();

            match end {
                SessionEnd::Shutdown => break 'session,
                SessionEnd::Recreate => {
                    log::info!(
                        target: "moss::build::watch",
                        "Recreating the file watcher for '{}' — the next sweep pass covers the re-registration gap",
                        folder_path
                    );
                    continue 'session;
                }
                SessionEnd::ChannelClosed => {
                    // No hot recreate on our own initiative: a debouncer whose
                    // thread dies instantly would otherwise spin this loop.
                    // The sweep keeps the preview correct meanwhile, and its
                    // strike rule is the bounded, evidence-driven retry.
                    log::error!(
                        "❌ File watcher channel closed for '{}' — sweep-only until the strike rule recreates it",
                        folder_path
                    );
                    tokio::select! {
                        _ = &mut shutdown_rx => break 'session,
                        _ = health.recreate_requested() => continue 'session,
                    }
                }
            }
        }

        // The worker exits with its watcher TASK (not with any one watcher
        // session): it finishes any in-flight build first (the build is
        // spawn_blocking and unkillable — never aborted), then drops whatever
        // is still queued, which is right at folder close.
        worker_handle.request_shutdown();
        worker::deregister(&folder_path, &worker_handle);
        supervision::deregister(&folder_path, &health);
    })));
}

/// Handle one batch of debounced events from `notify-debouncer-full`.
///
/// Each batch carries the raw events that turned `DEBOUNCE_MS` old since the
/// last drain — a delay, not a quiet period, so two events 300ms apart arrive
/// as two batches 300ms apart (see `DEBOUNCE_MS`). The crate has already
/// dropped same-kind duplicates within a batch and stitched rename pairs into
/// `Modify(Name(Both))` events (see module doc-comment for the platform table).
///
/// Pipeline:
/// 1. Filter the batch to events that touch a watchable path
///    (gitignore-style: skip dotfiles/dirs, `node_modules`, auto-managed
///    `.moss/*` subdirs; allowlist user-editable `.moss/*` files).
/// 2. Drop events whose kind shouldn't trigger a rebuild (AccessTime,
///    Metadata-only on a directory — iCloud materialization noise, etc.).
/// 3. Collect content-hash-gate-eligible paths (the worker runs the actual
///    stat+SHA-256 check at admission — the pump never reads file contents).
/// 4. Extract rename pairs from `Modify(Name(Both))` events for the rebuild
///    event's `moved_output_paths` field.
/// 5. Hand the request to the host's [`RebuildDispatch`].
async fn handle_debounced_batch(
    events: Vec<DebouncedEvent>,
    folder_path: &str,
    emit: &EventRelay,
    dispatch: &RebuildDispatch,
) {
    // ── DEDICATED SOURCE-ASSET PASS ──────────────────────────────────────────
    // Runs BEFORE the rebuild-eligibility gate so that asset edits/deletes/
    // renames always notify the editor — even when the content-hash gate would
    // suppress the rebuild (e.g. metadata-only touch, cloud-sync re-emit, or a
    // change the gate considers unchanged) or the post-rebuild output diff
    // suppresses `FileChanged`. The editor renders source bytes directly via
    // `moss-source://` URLs; `AssetReady` is a build-OUTPUT signal and cannot see
    // in-place source edits. This pass is the authoritative source-change signal.
    //
    // Not just images: an external Finder move of a non-image asset, or of a
    // FOLDER containing assets, delivers watcher events whose paths carry no
    // image extension — previously nothing was emitted here, so the editor's
    // reference resolver revalidated only if a `FileChanged` happened to follow,
    // and stayed stale forever when the rebuild was gated out or its output
    // diff came up empty. See `source_asset_request_paths` for the event policy
    // and docs/archive/2026-08-14-site-settings-and-attachments.md (folded-in
    // bug) for the observed failure.
    let root_path = Path::new(folder_path);
    for ev in &events {
        for request_path in source_asset_request_paths(root_path, ev.kind, &ev.paths) {
            emit(MossEvent::SourceAssetChanged { path: request_path });
        }
    }
    // ── END SOURCE-ASSET PASS ────────────────────────────────────────────────

    // ── RAW CREATE PASS (source-agnostic green flash) ─────────────────────────
    let raw_creates = collect_raw_create_keys(&events, root_path, |p| p.is_dir());
    if !raw_creates.is_empty() {
        emit(MossEvent::RawFileCreated { paths: raw_creates });
    }
    // ── END RAW CREATE PASS ───────────────────────────────────────────────────

    // The OS admits it dropped events. FSEvents raises `kMustScanSubDirs` when
    // its buffer overflows, and a mass cloud materialization — 724 files
    // landing at once, the case this whole design exists for — is exactly the
    // burst that overflows it. `notify` reports it as a flag on an `Other`
    // event with no paths, which every filter below discards, so the entire
    // burst it stands for would vanish silently. Rebuild from a full scan
    // instead: `Full` re-reads the vault, which IS the reconciliation.
    if watcher_lost_events(&events) {
        log::info!(
            target: "moss::build::watch",
            "Watcher reported dropped events — rebuilding from a full scan"
        );
        // A confessed loss is health evidence, not a strike (phase 3): the
        // watcher is alive and honest about an overflow, and the full-scan
        // rebuild below is the whole remedy (Watchman's answer to overflow is
        // rescan-and-keep-the-watch; ours is the same). The ledger note keeps
        // the sweep's strike rule from reading the missed events as a death.
        if let Some(h) = supervision::get(folder_path) {
            h.note_confession();
        }
        dispatch(worker::RebuildRequest::full()).await;
        return;
    }

    // Content-hash gate paths, collected instead of checked (phase 1b): the
    // pump must not read file contents, so gate-eligible Modify events ride
    // the request and the WORKER runs the stat+SHA-256 check at admission —
    // a gate that suppresses after the handoff costs one no-op admission; a
    // gate that wedges no longer takes the pump down.
    let mut deferred_gate_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut all_gate_eligible = true;

    let mut actionable: Vec<&DebouncedEvent> = Vec::with_capacity(events.len());
    for ev in &events {
        if !any_path_watchable(root_path, &ev.paths) {
            continue;
        }
        // The root-relative question `path_is_watchable` cannot ask: is every
        // path in this event something moss wrote? A root `AGENTS.md` is, and
        // the non-recursive root watch on Linux/Windows still hears it (#960).
        if scope::all_paths_moss_written(root_path, &ev.paths) {
            continue;
        }
        // Nested-vault boundary: a subtree owning its own `.moss/` is a
        // different site's territory — the outer vault never rebuilds on it.
        if scope::all_paths_in_nested_vault(root_path, &ev.paths) {
            log::debug!(
                target: "moss::build::watch",
                "Skipped {} path(s) inside a nested moss site: {:?}",
                ev.paths.len(),
                ev.paths
            );
            continue;
        }
        if !should_recompile_for_event(ev.kind, &ev.paths) {
            log::trace!(
                target: "moss::build::watch",
                "Suppressed {:?} on {} path(s): {:?}",
                ev.kind,
                ev.paths.len(),
                ev.paths
            );
            continue;
        }
        if !any_path_passes_filter(root_path, &ev.paths) {
            continue;
        }
        match pump_gate(ev.kind, folder_path, &ev.paths) {
            PumpGate::Suppress => {
                log::debug!(
                    target: "moss::build::watch",
                    "Suppressed on the pump: {} agent-config path(s) ({:?})",
                    ev.paths.len(),
                    ev.kind
                );
                continue;
            }
            PumpGate::Proceed => {
                all_gate_eligible = false;
            }
            PumpGate::DeferHashCheck => {
                deferred_gate_paths.extend(ev.paths.iter().cloned());
            }
        }
        actionable.push(ev);
    }

    if actionable.is_empty() {
        return;
    }

    // PATTERN 6 — INODE+FILE-ID PAIRING via notify-debouncer-full
    // Extract source-relative rename pairs from stitched `Modify(Name(Both))`
    // events. `path_to_relative_key` canonicalizes both sides; for the `from`
    // path (now gone) it falls back to the raw path, which the watcher emits
    // in canonical form on macOS.
    let rename_pairs = extract_rename_pairs(&actionable, folder_path);

    // Log which file(s) triggered the rebuild for easier debugging.
    let trigger_files: Vec<String> = actionable
        .iter()
        .flat_map(|e| e.paths.iter())
        .filter(|p| path_passes_filter(root_path, p))
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    // [diag] energy: append inter-arrival so a rebuild loop is obvious at a
    // glance (repeated small deltas) rather than requiring timestamp math.
    let since = rebuild_interarrival()
        .map(|d| format!("{}ms since last", d.as_millis()))
        .unwrap_or_else(|| "first this session".to_string());
    // [diag] The kinds, and whether the worker's content-hash gate will even
    // be consulted. `gate_paths` is `None` as soon as ONE actionable event was
    // `PumpGate::Proceed`, so without this a log with no "Gated at admission"
    // line is ambiguous three ways: the gate ran and the bytes differed, it
    // ran and failed open, or it never ran. Distinct kinds only — a batch of
    // twenty writes is twenty identical kinds and one is as informative.
    let mut kinds: Vec<String> = Vec::new();
    for ev in &actionable {
        let kind = format!("{:?}", ev.kind);
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    log::info!(
        "Rebuild triggered by: {:?} ({} event(s) {:?}, {}, {} rename pair(s)) [{}]",
        trigger_files,
        actionable.len(),
        kinds,
        if all_gate_eligible {
            "hash-gated"
        } else {
            "unconditional"
        },
        rename_pairs.len(),
        since
    );

    let trigger = classify_trigger(root_path, &actionable);
    dispatch(worker::RebuildRequest {
        rename_pairs,
        trigger,
        // Suppressible only when EVERY actionable event deferred to the
        // hash gate; one unconditional event (create/remove/lost-scan)
        // makes the whole batch unconditional.
        gate_paths: all_gate_eligible.then_some(deferred_gate_paths),
    })
    .await;
}
