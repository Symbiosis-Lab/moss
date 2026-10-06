//! The seal's generation-dependent half: materializing a generation on disk
//! and everything that can only run once that has happened.
//!
//! [`crate::build::advertise_sealed`] (`build.rs`) does the per-build half —
//! repairing `stage_dir`, writing `hashes.json`, emitting `AssetsSettled` —
//! and hands off a [`PendingSeal`] here instead of materializing inline. On a
//! long-lived process (the app, or headless `moss build --serve --watch`)
//! that hand-off goes through [`request`], which debounces it: `ship_phase`
//! clones every sealed entry (COW on APFS, but still one syscall and one
//! directory-entry creation per file) into `generations/<id>/`, and inside a
//! cloud-synced vault the OS file-provider daemon's own bookkeeping on top of
//! that dwarfs the actual copy — measured at roughly 9 CPU-seconds per 3,750
//! creates-plus-deletes. Paying that on every watch rebuild (every 5–13 s
//! autosave) is disproportionate to what one edit changed; paying it once per
//! idle pause is not.
//!
//! # What stays true regardless of when this runs
//!
//! The preview reads `stage_dir`, never a generation directly — so a pending
//! (not yet materialized) seal never blocks or delays what the author sees.
//! `current` always points at the last generation that WAS materialized: a
//! kill, a crash, or simply never reaching quiet leaves it there, pointing at
//! a complete, valid, previously-shipped tree — never at a half-written one,
//! because the pointer moves only after the copy has finished; a copy cut
//! off midway leaves it where it was. Generations remain independent inodes
//! (`fs::copy`, never a hardlink); this module changes WHEN that copy
//! happens, never the copy's own shape.
//!
//! # Forcing it
//!
//! [`settle`] is the one place outside this module that should ever need to
//! know a debounce exists here. It runs the pending materialize right now,
//! on the caller's task, and returns only once `current` (if anything was
//! pending) has caught up. Call it before anything that reads
//! `current_generation_id()` and needs it to match the latest edit: a deploy
//! (both the moss-hosted and the plugin/OnionPress routes), a folder close or
//! switch (complete the pending seal rather than abandon it — the desktop
//! shell's own folder-session teardown calls this), an app quit, or a
//! syndication/plugin read of `read_site_file` (which resolves under
//! `current_ptr()`; see `plugins/project_files.rs`'s own note on why it does
//! not need a separate fix — it runs behind the same publish gate `settle`
//! already covers). `search_lane::settle_for_publish` remains the search
//! index's own sync point and is unaffected here — deploy calls both.
//!
//! Ending a folder session drops its lane: `FolderSession::shutdown` calls
//! [`evict`]. A caller that ends a session settles it first (the desktop
//! shell's close and switch paths both do), so the lane is idle by then.
//! Without eviction a folder opened and closed repeatedly would leak one
//! parked lane per open.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use crate::build::debounce::{HandlerFuture, Lanes};
use crate::build::feeds::search_lane::Freshness;
use crate::build::manifest::SealedManifest;
use crate::build::ship::ShipVerdict;
use crate::moss_paths::MossPaths;
use crate::system::folder_session::{DerivedWorkGate, FolderSession};

/// Quiet period a sealed generation must survive before it materializes. Same
/// value as the search lane's own (pre-extraction) window — long enough that
/// a 5–13 s autosave cadence collapses to one materialize per typing pause.
pub const IDLE: std::time::Duration = std::time::Duration::from_secs(20);

/// Upper bound on deferral under sustained saving. Without it a vault edited
/// continuously would never materialize a generation at all.
pub const MAX_DEFER: std::time::Duration = std::time::Duration::from_secs(120);

// Test-only: a sleep inserted before `materialize_and_promote`, inside the
// blocking closure, so a test can hold a materialize open. Task-local so
// concurrent tests never see each other's delay; read on the calling task
// and moved into the closure, since task-locals do not reach
// `spawn_blocking`'s thread.
#[cfg(test)]
tokio::task_local! {
    pub(crate) static MATERIALIZE_TEST_DELAY: std::time::Duration;
}

/// Everything [`run_materialize_phase`] needs, handed off whole by
/// `advertise_sealed` once the per-build half is done. Not `Clone`: it owns
/// `cache_lease`, a one-shot RAII lease that must drop exactly once (see
/// `build.rs`'s `SealGuards` for why), which is also why the debouncer this
/// travels through (`build::debounce::Lanes`) does not require its payload
/// to be `Clone` — see that module's own doc on `Slot`.
pub(crate) struct PendingSeal {
    pub sealed: SealedManifest,
    pub verdict: ShipVerdict,
    pub ports: crate::build::SealPorts,
    /// The exact key `folder_session::registry()` registers under — carried
    /// as an owned `String` because this phase may run long after the call
    /// that built it, on a task that does not borrow from it.
    pub folder_path: String,
    pub stage_dir: std::path::PathBuf,
    /// When this generation was sealed — NOT when materialize actually runs.
    /// The "N ms after seal" log line this feeds now reports the debounce
    /// wait too, which is deliberate: it is the number this whole module
    /// exists to shrink the *count* of, not to hide.
    pub sealed_at: std::time::Instant,
    pub promotion_epoch: u64,
    /// `promotion_epoch`'s cross-process-comparable counterpart — wall-clock
    /// nanos, since `promotion_epoch` is process-local and cannot be compared
    /// against a second `moss` process's own admission. Read in this phase,
    /// against `infra::folder_lock::last_promoted_epoch`, immediately before
    /// `materialize_and_promote`.
    pub admission_nanos: u64,
    pub render_seq: Option<u64>,
    pub freshness: Freshness,
    pub is_pinned: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    pub session: Option<Arc<FolderSession>>,
    pub cache_lease: Option<crate::build::lifecycle::CacheWriteLease>,
    pub build_root_identity: Option<crate::build::lifecycle::root_identity::RootIdentity>,
}

/// One debounce lane per open folder (`.moss` directory), keyed the same way
/// `search_lane`'s own lanes used to be. While the folder's derived-work gate
/// is closed (nobody is looking) a quiesced seal stays pending instead of
/// materializing; see `system/folder_session/derived_work.rs`. The cache
/// sweep waits with it, since a pending seal holds its cache lease.
static LANES: LazyLock<Lanes<PendingSeal>> = LazyLock::new(|| {
    Lanes::new(IDLE, MAX_DEFER, dispatch_materialize_phase).with_gate(seal_gate)
});

fn seal_gate(req: &PendingSeal) -> DerivedWorkGate {
    let gate = DerivedWorkGate::of(req.session.as_ref());
    if gate.is_closed() {
        log::debug!("materialize deferred until the window is visible again");
    }
    gate
}

/// Plain-fn adapter `build::debounce::Handler<PendingSeal>` needs: `Lanes`
/// stores a function pointer (see its own doc), so the actual async body
/// lives in [`run_materialize_phase`] and this just boxes the call.
fn dispatch_materialize_phase(req: PendingSeal) -> HandlerFuture {
    Box::pin(run_materialize_phase(req))
}

/// Hand `req` to `key`'s debounce lane. Non-blocking: publishes the request
/// and returns. Used by the long-lived arm (the app, or headless `moss build
/// --serve --watch`) — the CLI/one-shot arm calls [`run_now`] instead,
/// unchanged from before this split (it keeps sealing synchronously: that
/// caller drops the tokio runtime as soon as the build returns, so a
/// debounced materialize would never get to run).
pub(crate) fn request(key: &Path, req: PendingSeal) {
    LANES.request(key, req);
}

/// Run the materialize phase immediately, bypassing the debounce lane
/// entirely. The CLI/one-shot call site's own entry point.
pub(crate) async fn run_now(req: PendingSeal) {
    run_materialize_phase(req).await;
}

/// Materialize the pending generation for `mp`'s folder right now, if one is
/// waiting, and wait for it to finish. A no-op when nothing is pending —
/// including when the background lane already got to it first.
///
/// Call this before anything that needs `current_generation_id()` to match
/// the latest sealed manifest: see this module's own doc for the standing
/// list of call sites (deploy, folder close/switch, app quit).
pub async fn settle(mp: &MossPaths) {
    LANES.settle_now(mp.root()).await;
}

/// [`settle`] every folder with a pending seal, not just one. For a folder
/// switch: moss enforces one open folder at a time, so in practice this
/// completes whichever single folder was active before the switch, without
/// the switch handler having to know or look up which one that was — see
/// this module's own doc on why "complete rather than abandon" matters here
/// specifically (a session's `cancel` firing does not stop its pending
/// lane; only a process exit before this runs would lose the work, which is
/// the app-quit call site's own reason for calling `settle`/`settle_all`
/// too).
pub async fn settle_all_pending() {
    LANES.settle_all().await;
}

/// Tear down `folder`'s debounce lane and stop its background worker. Called
/// from `FolderSession::shutdown`, which runs after the session's pending
/// seal was settled; a lane that is still busy (a seal pending or running)
/// is left alone rather than have its work discarded — see `Lanes::evict`.
pub(crate) fn evict(folder: &Path) {
    LANES.evict(MossPaths::new(folder).root());
}

/// The materialize phase itself — everything `advertise_sealed` used to run
/// after `repair_staged_html`, moved here verbatim except for the parts that
/// used to short-circuit on `owns_shared`/`mat_ok` and now read `req.verdict`
/// (already final) plus the `Promotion` this phase itself computes.
async fn run_materialize_phase(req: PendingSeal) {
    let PendingSeal {
        sealed,
        verdict,
        ports,
        folder_path,
        stage_dir,
        sealed_at,
        promotion_epoch,
        admission_nanos,
        render_seq,
        freshness,
        is_pinned,
        session,
        cache_lease,
        build_root_identity,
    } = req;

    let mp = MossPaths::new(Path::new(&folder_path));
    let announcer = ports.announcer.as_ref();
    let reporter = ports.events.as_ref();
    let sealed_as = sealed.generation_id().to_string();

    // Re-acquire the per-folder stage-write guard, freshly — see
    // `advertise_sealed`'s own doc for why it is not carried across from
    // there. Held only for this phase's own span: the `ship_phase` read of
    // `stage_dir` plus the generation GC and search-lane request that follow,
    // all of which depend on the generation `ship_phase` is about to freeze.
    let _stage_write_guard = match &session {
        Some(s) => Some(s.lock_stage_write().await),
        None => None,
    };

    // Copy stage_dir → generations/<gen-id>/ and swap `current`. `mat_ok`
    // gates advertisement to deploy: a failed materialize must NOT publish a
    // manifest whose generation_dir is partial/absent. `Superseded` is the
    // third outcome: a NEWER build already promoted, so this phase's swap was
    // refused — not an error, but not `mat_ok` either, since advertising this
    // older manifest to deploy would roll the published site back exactly as
    // the symlink swap would have. Under the debounce this lane runs behind,
    // `Superseded` should be rare in the single-writer case (the lane always
    // materializes the LATEST sealed manifest, never a stale one it lost a
    // race against) — it stays possible only against a genuine second writer
    // (a forced `settle` racing the background pass, or two moss processes on
    // the same folder), both pre-existing possibilities this phase inherits
    // rather than introduces.
    //
    // `spawn_blocking`, not inline: `materialize_and_promote` is synchronous
    // (a copy loop with no `.await`), and a synchronous call inside this
    // task's poll cannot be cut off by a `tokio::time::timeout` wrapped
    // around `settle`/`settle_all_pending` — the timeout's own poll is stuck
    // inside the call until it returns. An app that bounds its quit on a
    // settle needs the copy on its own thread so the timeout can fire.
    // `sealed` moves into the closure and back out, rather than being
    // cloned, because it may hold CAS-backed bytes.
    //
    // If that timeout fires and the process exits mid-copy, `current` is
    // untouched (only `lifecycle::promote`, after `ship_phase` returns, moves
    // it), so the previous generation keeps serving. The half-written
    // generation directory keeps its `store_gc::GenerationWriteLock` file,
    // now unlocked, and a later generation GC removes it. A caller that
    // times out and does NOT exit leaves the copy to finish and promote on
    // its own, outside the stage-write guard, so only a quit should bound a
    // settle.
    use crate::build::ship::Promotion;
    #[cfg(test)]
    let materialize_test_delay = MATERIALIZE_TEST_DELAY.try_with(|d| *d).ok();
    let folder_path_for_materialize = folder_path.clone();
    // Cross-PROCESS counterpart of the stage-write guard just re-acquired
    // above — see `infra::folder_lock`'s module doc for why its own window,
    // freshly taken HERE rather than carried over from `advertise_sealed`
    // (which ran this build's per-build half, possibly long before the
    // debounce fired this phase).
    let folder_build_lock = crate::infra::folder_lock::acquire_async(Path::new(&folder_path)).await;
    // Order, not just exclusion: a slower phase can land here after a newer
    // admission already promoted, which `promotion_epoch` (process-local)
    // can't catch — compare `admission_nanos` against the cross-process record.
    let cross_process_verdict: Option<Promotion> = folder_build_lock.as_ref().and_then(|_| {
        let persisted = crate::infra::folder_lock::last_promoted_epoch(Path::new(&folder_path));
        (admission_nanos <= persisted).then(|| {
            log::info!("promotion refused (cross-process): epoch {admission_nanos} <= persisted {persisted}");
            Promotion::Superseded
        })
    });
    let blocking_result = tokio::task::spawn_blocking(move || {
        #[cfg(test)]
        if let Some(delay) = materialize_test_delay {
            std::thread::sleep(delay);
        }
        let mp = MossPaths::new(Path::new(&folder_path_for_materialize));
        let promotion = match cross_process_verdict {
            Some(superseded) => Ok(superseded),
            None => crate::build::ship::materialize_and_promote(
                &sealed,
                &mp,
                &stage_dir,
                promotion_epoch,
                render_seq,
                verdict,
            ),
        };
        // Record a real promotion before the lock drops, so read+promote+write
        // land in one held window.
        if matches!(promotion, Ok(Promotion::Promoted)) {
            if let Err(e) = crate::infra::folder_lock::record_promoted_epoch(Path::new(&folder_path_for_materialize), admission_nanos) {
                log::error!("run_materialize_phase: could not record the cross-process promotion epoch: {e}");
            }
        }
        drop(folder_build_lock);
        (sealed, promotion)
    })
    .await;
    let (mut sealed, promotion) = match blocking_result {
        Ok(pair) => pair,
        Err(e) => {
            // The copy panicked and `sealed` went with it: nothing is left
            // to adopt or collect.
            log::error!("run_materialize_phase: materialize task panicked: {e}");
            drop(_stage_write_guard);
            return;
        }
    };
    match &promotion {
        Ok(Promotion::Promoted) => log::info!(
            "promoted gen {} (sealed as {}, epoch {}, render #{}, {} files) {} ms after seal",
            sealed.generation_id(),
            sealed_as,
            promotion_epoch,
            render_seq.map_or("none".to_string(), |r| r.to_string()),
            sealed.files().len(),
            sealed_at.elapsed().as_millis()
        ),
        Ok(Promotion::Superseded) => log::info!(
            "run_materialize_phase: generation {} was superseded before promotion",
            sealed.generation_id()
        ),
        // Said once, with its reason, by `materialize_and_promote`.
        Ok(Promotion::Withheld(_)) => {}
        Err(_) => log::error!("run_materialize_phase: materialize failed — current_ptr stays put"),
    }
    // Observed at the plain path on purpose: `build_dir()` follows the
    // handle, so it can never disagree with the start identity.
    crate::build::lifecycle::root_identity::log_build_root(
        &mp.root().join("build.nosync"),
        "ship",
        build_root_identity,
    );
    let mat_ok = matches!(promotion, Ok(Promotion::Promoted));
    let owns_shared = crate::build::ship::tail_owns_shared_state(&promotion);

    // `materialize_and_promote` (ship_phase) was the last thing above that
    // could still read a CAS blob a concurrent GC might collect, so the lease
    // has done its job — drop it now, before the GC step below can trigger
    // cache GC.
    //
    // Test-only: hand `cache_lease_ship_tests.rs` the writer count exactly
    // here, still inside the window the fix's whole point is to hold open. A
    // no-op outside that test's `SHIP_PHASE_LEASE_SAMPLE` scope.
    #[cfg(test)]
    let _ = crate::build::SHIP_PHASE_LEASE_SAMPLE.try_with(|sample| {
        sample.store(crate::build::lifecycle::snapshot(&mp).2, std::sync::atomic::Ordering::SeqCst);
    });
    drop(cache_lease);

    // Nothing reads the held bytes past `materialize_and_promote`, and
    // `sealed` is about to be adopted as the manifest deploy keeps for the
    // folder's lifetime.
    sealed.release_held();

    // Say it out loud, and only for a real swap: this instant is when the
    // user's change became what a generation-reading consumer (deploy, a
    // plugin's `read_site_file`) sees — never the live preview, which reads
    // `stage_dir` and already saw it back in `advertise_sealed`.
    if mat_ok {
        announcer.promoted(sealed.generation_id());
    }

    // Retain generations + sweep the cache. Non-fatal. Generation GC stays
    // inside `_stage_write_guard`'s span, which serializes it against the
    // next rebuild's promotion; the cache sweep runs only when no build or
    // encode holds a cache lease (`store_gc::maybe_gc_cache`). Blocking work,
    // so it goes to the blocking pool.
    if mat_ok {
        // `project_root()`, not `root()`: `MossPaths::new` appends `.moss`,
        // so handing it the `.moss` dir builds paths under `.moss/.moss/` and
        // the collector sweeps an empty tree.
        let project_root = mp.project_root().to_path_buf();
        let gen_id = sealed.generation_id().to_string();
        // Resolve the pin set HERE, on this task: `is_pinned` may borrow
        // AppState and cannot cross into the blocking pool.
        let pinned: std::collections::HashSet<String> =
            crate::build::store_gc::list_generations(&mp.generations_dir())
                .into_iter()
                .filter(|g| is_pinned(g) || crate::build::feeds::search_lane::is_indexing(&mp, g))
                .collect();
        let _ = tokio::task::spawn_blocking(move || {
            let mp = MossPaths::new(&project_root);
            crate::build::collect_build_store(&mp, &gen_id, &pinned);
        })
        .await;
        // Hand the FROZEN generation to the search lane — staging is wrong,
        // the next build rewrites it. Nothing here awaits: the lane no longer
        // debounces its own request (this phase already did), so it starts
        // indexing immediately rather than stacking a second idle wait on top
        // of this one. `Freshness::Now` (a process that exits when the build
        // returns) already indexed synchronously inside the build.
        use crate::build::feeds::search_lane as lane;
        if matches!(freshness, lane::Freshness::Lane) && lane::enabled_for(&mp) {
            let want = lane::PageSet::of(&sealed.site_hashes_view().files);
            lane::request(&mp, sealed.generation_id(), want);
        }
    }

    // Release the stage-write guard now — everything below (advertising the
    // manifest, progress events, the backfill network ask) doesn't touch the
    // filesystem, so there's nothing left to protect.
    drop(_stage_write_guard);

    // Classify what publishing this generation would change on the live
    // site, against the record of the last publish that landed — or, with no
    // record, against the last deployed generation still on disk
    // (`manifest::backfill`). `tail_speaks` decides whether this phase may
    // describe a publish at all.
    use crate::build::manifest::backfill::{self, SealVerdict};
    let backfill_verdict =
        backfill::for_seal(&mp, backfill::tail_speaks(&sealed, mat_ok, owns_shared)).await;
    // Computed once per build by `advertise_sealed`, which always runs first.
    let removed = backfill_verdict
        .as_ref()
        .map(|_| crate::system::build_records::records().removed_addresses(&folder_path).unwrap_or_default());
    let server_ask = match (&backfill_verdict, &ports.server_diff) {
        (Some(SealVerdict::AskServer), Some(_)) => {
            Some((sealed.files().clone(), sealed.generation_id().to_string()))
        }
        _ => None,
    };

    // Advertise the sealed manifest to deploy ONLY when its generation
    // materialized on disk (mat_ok).
    if mat_ok {
        announcer.adopt_sealed(sealed).await;
    }

    // Stash the change set as a read model for webviews that boot between
    // builds, then push it to the ones already listening. A phase with
    // nothing to say does neither, and does not CLEAR: the manifest just
    // advertised (if any) is still publishable and the standing stash
    // describes it.
    let change_set = match backfill_verdict {
        None => None,
        Some(SealVerdict::Ready(set)) => Some(set),
        Some(SealVerdict::AskServer) => Some(match (&ports.server_diff, server_ask) {
            (Some(port), Some((files, gen_id))) => backfill::from_server(port, files, gen_id).await,
            _ => Default::default(),
        }),
    };
    // The removed addresses ride on whichever set resulted, classified or not.
    let change_set = change_set.zip(removed).map(|(set, removed)| set.with_removed(removed));
    announcer.publish_change_set(mp.project_root(), change_set).await;

    // Emit the "Sealed" progress tick (mat_ok-gated) — the counterpart of the
    // "Preparing..." tick the seal task reports before this phase starts.
    reporter.report(&crate::build::progress::PipelineEvent::BackgroundProgress {
        task: "sealing".to_string(),
        current: 1,
        total: 1,
        message: if mat_ok { "Sealed".to_string() } else { "Sealed (materialize failed)".to_string() },
        completed: true,
        advisories: vec![],
    });
}

#[cfg(test)]
#[path = "seal_phase_tests.rs"]
mod tests;
