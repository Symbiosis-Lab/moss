//! BackgroundHandle: typed materialization barrier for the deferred phase.
//!
//! `BackgroundHandle::spawn(carry_forward, f)` constructs a coordinator + sender,
//! invokes the closure to register workers (giving them the sender + a JoinSet),
//! and spawns a coordinator task that drains the channel into a sealed manifest.
//!
//! `await_completion` joins all workers, then awaits the coordinator. The
//! returned `SealedManifest` is the only path to a deploy-ready manifest —
//! callers that take `&SealedManifest` cannot run before this barrier resolves.
//!
//! See: moss#524 (single emit API), moss#552 (debug-assert blocking_keys ⊆ files).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

use crate::build::coordinator::{EmitMessage, ManifestCoordinator};
use crate::build::manifest::{PendingManifest, SealedManifest};
use crate::tasks::{TaskId, TaskRegistry};
use crate::types::content::SiteHashes;

// ---------------------------------------------------------------------------
// BuildTerminalBarrier
// ---------------------------------------------------------------------------

/// Owns **every terminal receipt a finished build owes its listeners** — the
/// parent Build Job's terminal state, and the typed `BuildComplete` event.
///
/// One concern, one owner. Both receipts answer the same question ("this build
/// is over"), so they are emitted from the same place: the SINGLE post-join
/// point in `await_completion`, after the coordinator seal.
///
/// History, because it repeats: both receipts used to hang off the video
/// worker's `finish_build`. The Job half was moved here first (Step 3 Phase 4,
/// FIX 2) because that coupling leaked the parent on images-only,
/// images-finish-last and epoch-superseded builds. The `BuildComplete` half was
/// left behind, and rotted the same way for the same reason — worse, in fact:
/// `pipeline.rs` only spawns the video worker when the site HAS videos, so on a
/// site with images and no videos the event was never emitted at all, and the
/// preview never refreshed after its first build (LOG-C2CF-T0935-08-14).
/// Anything that means "the build finished" belongs here, not on a worker that
/// may not run.
///
/// Two terminal paths, mutually exclusive:
/// - **Success** — `terminate_succeeded()`, called once from `await_completion`
///   after every media worker has joined AND the coordinator has sealed. Drives
///   the "Built · N pages · X.Xs" Job receipt and emits `BuildComplete`.
/// - **Cancel/leak guard** — `Drop`. If the handle is dropped WITHOUT
///   `await_completion` resolving the success path (fire-and-forget drop, an
///   early-return/supersede that never awaited), any spawned-but-not-terminal
///   parent is driven to `Cancelled` so no `Running` parent leaks (a leaked
///   `Running` parent never prunes — `prune_terminal` only removes terminals).
///   **No `BuildComplete` on this path**: a build that never reached the seal
///   did not finish, and telling the frontend otherwise would refresh the
///   preview onto a half-written stage.
///
/// Reads the shared `build_parent` cell at terminal time (not at construction):
/// the parent may not exist yet when the barrier is built, since it is minted
/// lazily inside the workers. A content-only rebuild leaves the cell `None`, so
/// the Job half no-ops — zero Jobs (invariant #6). The `BuildComplete` half
/// still fires: "no media ran" and "no build finished" are different facts.
pub struct BuildTerminalBarrier {
    /// `None` in headless mode — no Tauri runtime, so no Job registry. The
    /// `BuildComplete` half is independently gated on `reporter`.
    registry: Option<Arc<TaskRegistry>>,
    build_parent: Arc<Mutex<Option<TaskId>>>,
    page_count: Arc<std::sync::atomic::AtomicUsize>,
    /// A headless build's reporter discards, so this is carried rather than
    /// branched on.
    reporter: Arc<dyn crate::build::ports::reporter::BuildReporter>,
    /// Written by the video worker as it converts; read here. Post-join the
    /// write has happened-before, which is more than the old in-worker read
    /// could say for `skipped_symlinks` (a peer worker's counter).
    videos_converted: Arc<std::sync::atomic::AtomicU32>,
    skipped_symlinks: Arc<std::sync::atomic::AtomicU32>,
    start_time: Instant,
    /// Set once the success terminal has run, so `Drop` knows not to also try
    /// (and so a double-`terminate_succeeded` is a no-op).
    settled: bool,
}

impl BuildTerminalBarrier {
    /// Build the barrier from the build's shared services. Returns `None` only
    /// when there are no services at all (pure-Rust manifest tests) — headless
    /// still gets one, and its two halves no-op individually.
    pub fn from_services(
        services: Option<&crate::types::services::BuildServices>,
        start_time: Instant,
    ) -> Option<Self> {
        let s = services?;
        Some(Self {
            registry: s.task_registry.clone(),
            build_parent: s.build_parent.clone(),
            page_count: s.page_count.clone(),
            reporter: s.reporter.clone(),
            videos_converted: s.videos_converted.clone(),
            skipped_symlinks: s.skipped_symlinks.clone(),
            start_time,
            settled: false,
        })
    }

    fn parent(&self) -> Option<TaskId> {
        *self
            .build_parent
            .lock()
            .expect("BuildTerminalBarrier build_parent mutex poisoned")
    }

    /// Emit both terminal receipts. Idempotent — marks `settled` so the Drop
    /// guard skips and a double call is a no-op.
    ///
    /// The Job `Amount` is sourced from the render-phase `page_count` (=
    /// `html_total`); the span is the measured build time
    /// (`start_time.elapsed()`), which is the same clock `BuildComplete`
    /// reports as `total_time_ms`.
    pub fn terminate_succeeded(&mut self) {
        if self.settled {
            return;
        }
        self.settled = true;
        self.terminate_parent_job();
        self.emit_build_complete();
    }

    /// The parent Build Job's "Built · N pages · X.Xs" receipt. No-op when no
    /// parent was minted (content-only rebuild) or in headless mode.
    fn terminate_parent_job(&self) {
        let (Some(registry), Some(parent)) = (self.registry.as_ref(), self.parent()) else {
            return;
        };
        let page_count = self.page_count.load(std::sync::atomic::Ordering::Relaxed);
        registry.done_with_elapsed_by_id(
            parent,
            crate::tasks::Verb::core(crate::tasks::Verb::BUILT),
            Some(crate::tasks::Amount {
                count: page_count as u64,
                noun: "pages".to_string(),
            }),
            None,
            // The parent receipt is the "Built · N pages" header; per-media
            // advisories already flow into the media CHILD Jobs' terminal states.
            Vec::new(),
            self.start_time.elapsed(),
        );
    }

    /// The typed `BuildComplete` event — the frontend's "the build finished,
    /// refresh the preview" signal (`preview-manager.ts`). Fires on EVERY
    /// completed build, including one with no media at all: a text-only site
    /// still needs its preview refreshed off the new stage.
    ///
    /// Split from the emit so the payload is testable without a reporter that
    /// can carry it anywhere.
    fn build_complete_event(&self) -> crate::build::progress::PipelineEvent {
        use std::sync::atomic::Ordering::Relaxed;
        crate::build::progress::PipelineEvent::BuildComplete {
            videos_converted: self.videos_converted.load(Relaxed),
            total_time_ms: self.start_time.elapsed().as_millis() as u64,
            skipped_symlinks: self.skipped_symlinks.load(Relaxed),
        }
    }

    fn emit_build_complete(&self) {
        self.reporter.report(&self.build_complete_event());
    }
}

impl Drop for BuildTerminalBarrier {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Not settled via the success path: a superseded / early-returned /
        // fire-and-forget build. Terminalize any spawned-but-not-terminal parent
        // as Cancelled so no Running parent leaks. No-op when no parent exists or
        // it already reached a terminal state (don't clobber a Succeeded receipt).
        // Deliberately no `BuildComplete` here — see the type doc.
        let (Some(registry), Some(parent)) = (self.registry.as_ref(), self.parent()) else {
            return;
        };
        if !registry.is_terminal_by_id(parent) {
            registry.cancel_by_id(parent);
        }
    }
}

// ---------------------------------------------------------------------------
// BuildError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum BuildError {
    JoinError(tokio::task::JoinError),
    Worker(String),
    Io(std::io::Error),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JoinError(e) => write!(f, "worker task panicked: {}", e),
            Self::Worker(s) => write!(f, "worker failed: {}", s),
            Self::Io(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::JoinError(e) => Some(e),
            Self::Worker(_) => None,
            Self::Io(e) => Some(e),
        }
    }
}

impl From<tokio::task::JoinError> for BuildError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::JoinError(e)
    }
}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ---------------------------------------------------------------------------
// BackgroundHandle
// ---------------------------------------------------------------------------

/// **Drop semantics:** dropping `BackgroundHandle` aborts the outer JoinSet
/// workers and detaches the coordinator. Inner detached
/// `Spawner::spawn_blocking` tasks (the actual conversion work)
/// keep running and hold their `tx` clones until they finish; the coordinator
/// then seals normally but the SealedManifest is dropped. To preserve the
/// seal, await `await_completion()` before dropping. The build pipeline's
/// seal+persist side task does this — see `build::run_pipeline`.
pub struct BackgroundHandle {
    coordinator_join: JoinHandle<SealedManifest>,
    workers: JoinSet<Result<(), BuildError>>,
    /// Every terminal receipt this build owes its listeners. `None` only for
    /// callers that have no `BuildServices` at all (pure-Rust manifest tests).
    /// Driven to its success terminal once all workers join AND the coordinator
    /// seals in `await_completion`; its `Drop` guard cancels a leaked `Running`
    /// parent on any non-await path.
    terminal_barrier: Option<BuildTerminalBarrier>,
    /// The build's `lifecycle::CacheWriteLease`: while it is open, no other
    /// build may unlink from staging or GC the cache. `await_completion`
    /// hands it back to the caller instead of dropping it, so the seal tail
    /// (`build::advertise_sealed`) can keep holding it across
    /// `materialize_and_promote`/`ship_phase` — see that function for where
    /// it finally drops.
    cache_lease: Option<crate::build::lifecycle::CacheWriteLease>,
}

impl BackgroundHandle {
    /// Spawn the coordinator and let `f` register worker tasks.
    ///
    /// `f` receives:
    /// - `tx: mpsc::Sender<EmitMessage>` — clone for each worker; drop in worker
    ///   on completion. Channel close = seal trigger.
    /// - `workers: &mut JoinSet<Result<(), BuildError>>` — spawn worker tasks here
    ///   (NOT into the global tokio runtime; the JoinSet drives `await_completion`).
    ///
    /// The original sender (the one not given to workers) is dropped at the end of
    /// `f`, so the coordinator only blocks on workers' clones.
    pub fn spawn<F>(carry_forward: SiteHashes, f: F) -> Self
    where
        F: FnOnce(mpsc::Sender<EmitMessage>, &mut JoinSet<Result<(), BuildError>>),
    {
        let (coordinator, tx) = ManifestCoordinator::new(carry_forward);
        Self::spawn_inner(coordinator, tx, None, None, f)
    }

    /// Like `spawn` but seeds the coordinator from an already-populated
    /// `PendingManifest` (carry-forward from the render phase).
    ///
    /// Use this in `build_inner` after the blocking phase has registered all
    /// render-phase emits into `pending`. The deferred workers continue
    /// accumulating on top of the render-phase state, producing a single
    /// `SealedManifest` that covers both phases.
    ///
    /// **Ownership**: `pending` is consumed; its accumulated state becomes the
    /// initial state of the coordinator's `PendingManifest`.
    pub fn spawn_with_pending<F>(pending: PendingManifest, f: F) -> Self
    where
        F: FnOnce(mpsc::Sender<EmitMessage>, &mut JoinSet<Result<(), BuildError>>),
    {
        let (coordinator, tx) = ManifestCoordinator::from_pending(pending);
        Self::spawn_inner(coordinator, tx, None, None, f)
    }

    /// Like `spawn_with_pending` but also attaches the build's terminal barrier
    /// — the single owner of the parent Job receipt and the `BuildComplete`
    /// event. Driven to its success terminal once all workers join and the
    /// coordinator seals (`await_completion`); its `Drop` guard cancels a leaked
    /// `Running` parent on any non-await path.
    ///
    /// **Every build path that can finish should use this variant**, including
    /// the zero-worker one: a text-only build still owes its listeners a
    /// "finished" receipt.
    pub(crate) fn spawn_with_pending_and_terminal<F>(
        pending: PendingManifest,
        terminal_barrier: Option<BuildTerminalBarrier>,
        cache_lease: Option<crate::build::lifecycle::CacheWriteLease>,
        f: F,
    ) -> Self
    where
        F: FnOnce(mpsc::Sender<EmitMessage>, &mut JoinSet<Result<(), BuildError>>),
    {
        let (coordinator, tx) = ManifestCoordinator::from_pending(pending);
        Self::spawn_inner(coordinator, tx, terminal_barrier, cache_lease, f)
    }

    fn spawn_inner<F>(
        coordinator: ManifestCoordinator,
        tx: mpsc::Sender<EmitMessage>,
        terminal_barrier: Option<BuildTerminalBarrier>,
        cache_lease: Option<crate::build::lifecycle::CacheWriteLease>,
        f: F,
    ) -> Self
    where
        F: FnOnce(mpsc::Sender<EmitMessage>, &mut JoinSet<Result<(), BuildError>>),
    {
        let coordinator_join = tokio::spawn(coordinator.run_until_drained());

        let mut workers = JoinSet::new();
        f(tx, &mut workers);
        // The `tx` parameter passed into `f` is moved; if `f` only cloned it for
        // workers and didn't keep one, the original is dropped here. If `f`
        // forgot to drop it (e.g. moved it into a long-lived struct), workers
        // would never seal. That's a programming bug; document it.

        Self {
            coordinator_join,
            workers,
            terminal_barrier,
            cache_lease,
        }
    }

    /// Materialization barrier: await all workers, then await coordinator, then
    /// emit the build's terminal receipts.
    ///
    /// Returns the `SealedManifest` once every emit has been applied, together
    /// with the build's `CacheWriteLease` (if any) — taken out of `self`
    /// rather than dropped, so the caller can carry it into the seal tail
    /// (`build::advertise_sealed`), which still needs the cache to survive a
    /// concurrent GC through `materialize_and_promote`. Failure of any worker
    /// is propagated; partial completions are NOT sealed (the coordinator is
    /// aborted), and on that path the lease is simply dropped with `self` —
    /// a build that failed ships nothing, so nothing downstream still needs it.
    // `pub(crate)`, not `pub`: the return type now carries `CacheWriteLease`,
    // which is itself `pub(crate)` (rustc's private-interface lint is what
    // caught the mismatch) — and every real caller is inside this crate
    // anyway (`build.rs`'s seal+persist tasks).
    pub(crate) async fn await_completion(
        mut self,
    ) -> Result<(SealedManifest, Option<crate::build::lifecycle::CacheWriteLease>), BuildError> {
        // Join workers first. If any returns Err or panics, propagate.
        //
        // On the error path we return WITHOUT calling `terminate_succeeded`, so
        // `self` (carrying the still-unsettled `terminal_barrier`) is dropped here
        // and its Drop guard cancels any spawned-but-not-terminal parent — no
        // leaked Running parent, no false "Built" receipt on a failed build.
        while let Some(res) = self.workers.join_next().await {
            // res: Result<Result<(), BuildError>, JoinError>
            res.map_err(BuildError::from)??;
        }
        // The coordinator seal is the TRUE post-join barrier: in GUI mode the media
        // dispatch worker returns immediately after spawning the actual conversion
        // into a DETACHED `spawn_blocking` that keeps a `tx` clone, so the JoinSet
        // join above does NOT wait for the conversion. The coordinator's recv loop
        // only returns None once every `tx` clone (including the detached ones) has
        // dropped — i.e. once all conversions have finished and lazily minted the
        // parent. So await the seal BEFORE terminalizing.
        let sealed = self.coordinator_join.await?;
        // Now every media worker (including detached GUI conversions) is done →
        // emit the build's terminal receipts from this SINGLE post-join point.
        // Runs for EVERY build regardless of which worker finished last, or
        // whether any worker ran at all — which is exactly what the old
        // per-worker emit could not say.
        if let Some(barrier) = self.terminal_barrier.as_mut() {
            barrier.terminate_succeeded();
        }
        Ok((sealed, self.cache_lease.take()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::manifest::HashBucket;

    #[tokio::test]
    async fn await_completion_returns_sealed_manifest_after_all_workers() {
        let handle = BackgroundHandle::spawn(SiteHashes::default(), |tx, workers| {
            let tx1 = tx.clone();
            workers.spawn(async move {
                tx1.send(EmitMessage::File {
                    rel_path: "a.html".to_string(),
                    hash: "aaa".to_string(),
                    bucket: HashBucket::Files,
                    oid: None,
                })
                .await
                .unwrap();
                tx1.send(EmitMessage::File {
                    rel_path: "b.html".to_string(),
                    hash: "bbb".to_string(),
                    bucket: HashBucket::Files,
                    oid: None,
                })
                .await
                .unwrap();
                Ok::<_, BuildError>(())
            });
            let tx2 = tx.clone();
            workers.spawn(async move {
                tx2.send(EmitMessage::File {
                    rel_path: "c.html".to_string(),
                    hash: "ccc".to_string(),
                    bucket: HashBucket::Files,
                    oid: None,
                })
                .await
                .unwrap();
                Ok::<_, BuildError>(())
            });
            // Original tx dropped at end of closure
        });

        let (sealed, _lease) = handle.await_completion().await.expect("await_completion failed");

        assert_eq!(sealed.files().len(), 3);
        assert!(sealed.files().contains_key("a.html"));
        assert!(sealed.files().contains_key("b.html"));
        assert!(sealed.files().contains_key("c.html"));
    }

    #[tokio::test]
    async fn await_completion_propagates_worker_error() {
        let handle = BackgroundHandle::spawn(SiteHashes::default(), |_tx, workers| {
            workers.spawn(async move {
                Err::<(), _>(BuildError::Worker("simulated failure".to_string()))
            });
        });

        let result = handle.await_completion().await;
        assert!(result.is_err(), "worker error must propagate");
    }

    #[tokio::test]
    async fn await_completion_with_no_workers_returns_empty_sealed_manifest() {
        let handle = BackgroundHandle::spawn(SiteHashes::default(), |_tx, _workers| {
            // No workers spawned; tx dropped immediately
        });

        let (sealed, _lease) = handle
            .await_completion()
            .await
            .expect("await_completion failed");
        assert!(sealed.files().is_empty());
    }

    /// A portable tempdir under `target/test-tmp`, matching
    /// `epoch_ordering_tests.rs`'s own fixture.
    fn temp_moss_paths() -> (tempfile::TempDir, crate::moss_paths::MossPaths) {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("moss-cache-lease")
            .tempdir_in(&base)
            .unwrap();
        let mp = crate::moss_paths::MossPaths::new(tmp.path());
        (tmp, mp)
    }

    /// The fix this pins: `await_completion` used to DROP the build's
    /// `CacheWriteLease` as part of consuming `self`, which released it
    /// before the seal tail (`build::advertise_sealed`) even started —
    /// letting a concurrent cache GC collect a CAS blob the seal tail's
    /// `materialize_and_promote`/`ship_phase` still needed to read. It must
    /// instead hand the lease back to the caller so the seal tail can keep
    /// holding it. A regression back to the old drop-on-return behavior
    /// shows up here as the lease already being closed the instant
    /// `await_completion` resolves.
    #[tokio::test]
    async fn await_completion_returns_the_cache_lease_instead_of_dropping_it() {
        let (_tmp, mp) = temp_moss_paths();
        let lease = crate::build::lifecycle::cache_write_lease(&mp);
        assert_eq!(crate::build::lifecycle::snapshot(&mp).2, 1, "sanity: the lease is open");

        let handle = BackgroundHandle::spawn_with_pending_and_terminal(
            PendingManifest::new(SiteHashes::default()),
            None,
            Some(lease),
            |_tx, _workers| {
                // No workers — the point is the lease, not the manifest.
            },
        );

        let (_sealed, returned_lease) =
            handle.await_completion().await.expect("await_completion failed");

        assert_eq!(
            crate::build::lifecycle::snapshot(&mp).2, 1,
            "await_completion must hand the lease back, not drop it — the count must \
             still read 1 right after it resolves"
        );
        assert!(returned_lease.is_some(), "await_completion must return the lease, not consume it");

        drop(returned_lease);
        assert_eq!(
            crate::build::lifecycle::snapshot(&mp).2, 0,
            "and dropping the returned lease must still release it normally"
        );
    }

    #[tokio::test]
    async fn await_completion_aborts_coordinator_on_worker_panic() {
        let handle = BackgroundHandle::spawn(SiteHashes::default(), |_tx, workers| {
            workers.spawn(async move {
                panic!("simulated panic");
                #[allow(unreachable_code)]
                Ok::<_, BuildError>(())
            });
        });

        let result = handle.await_completion().await;
        assert!(matches!(result, Err(BuildError::JoinError(_))));
    }

    // ── BuildTerminalBarrier ──────────────────────────────────────────────────

    use crate::tasks::{TaskKind, TaskRegistry, TaskScope, TaskState, TaskTone, WindowId};
    use crate::types::services::BuildServices;
    use std::sync::atomic::Ordering;

    /// A headless BuildServices wired with a live registry and the shared lazy-
    /// parent cell, plus a `BuildTerminalBarrier` reading the same cells — the
    /// exact wiring `build_inner` produces.
    fn barrier_fixture() -> (Arc<TaskRegistry>, BuildServices, BuildTerminalBarrier) {
        let registry = Arc::new(TaskRegistry::new());
        let mut svc = BuildServices::headless();
        svc.task_registry = Some(registry.clone());
        let barrier = BuildTerminalBarrier::from_services(Some(&svc), Instant::now())
            .expect("services present");
        (registry, svc, barrier)
    }

    fn build_parent_task(registry: &TaskRegistry, id: TaskId) -> TaskState {
        registry
            .tasks(&WindowId::from("main"), TaskScope::Preview)
            .into_iter()
            .find(|t| t.id == id)
            .expect("parent task present")
            .state
    }

    #[test]
    fn images_only_build_terminalizes_the_parent() {
        // FIX 2: an images-only build (the video worker never finishes the parent)
        // must still leave the parent in a TERMINAL state, not Running. The image
        // worker lazily minted the parent (real work); the post-join barrier drives
        // its success terminal regardless of which worker finished last.
        let (registry, svc, mut barrier) = barrier_fixture();
        // Simulate the image worker minting the parent on real work.
        let parent = svc.ensure_build_parent().unwrap();
        assert!(matches!(
            build_parent_task(&registry, parent),
            TaskState::Running { .. }
        ));
        // The single post-join barrier drives the terminal.
        barrier.terminate_succeeded();
        assert!(
            matches!(build_parent_task(&registry, parent), TaskState::Succeeded { .. }),
            "an images-only build must terminalize the parent (not leak it Running)"
        );
    }

    #[test]
    fn epoch_superseded_build_leaves_no_running_parent() {
        // FIX 2: an overlapping build (epoch bump) supersedes the prior one, which
        // early-returns WITHOUT driving the success terminal — the barrier is dropped
        // un-settled. Its Drop guard must cancel the spawned parent so no Running
        // parent leaks (a leaked Running parent never prunes).
        let (registry, svc, barrier) = barrier_fixture();
        let parent = svc.ensure_build_parent().unwrap();
        // Superseded: barrier dropped without terminate_succeeded().
        drop(barrier);
        assert!(
            matches!(build_parent_task(&registry, parent), TaskState::Cancelled),
            "a superseded build must Cancel (not leak Running) the parent"
        );
        // No running parent remains anywhere in the registry.
        let any_running = registry
            .tasks(&WindowId::from("main"), TaskScope::Preview)
            .iter()
            .any(|t| t.kind == TaskKind::Build && t.is_running());
        assert!(!any_running, "no Running Build parent may remain after supersede");
    }

    #[test]
    fn barrier_drop_after_success_does_not_clobber_the_receipt() {
        // The Drop guard must NOT overwrite a Succeeded receipt with Cancelled when
        // the success path already ran (the common GUI await path: terminate then drop).
        let (registry, svc, mut barrier) = barrier_fixture();
        let parent = svc.ensure_build_parent().unwrap();
        barrier.terminate_succeeded();
        drop(barrier);
        assert!(
            matches!(build_parent_task(&registry, parent), TaskState::Succeeded { .. }),
            "Drop after a successful terminate must keep the Succeeded receipt"
        );
    }

    #[test]
    fn content_only_rebuild_barrier_is_a_noop_on_both_paths() {
        // Invariant #6: when no real media work minted a parent, BOTH the success
        // terminal and the Drop guard must no-op — zero Jobs.
        let (registry, _svc, mut barrier) = barrier_fixture();
        barrier.terminate_succeeded();
        drop(barrier);
        assert!(
            registry
                .tasks(&WindowId::from("main"), TaskScope::Preview)
                .is_empty(),
            "a content-only rebuild (no parent minted) must mint zero Jobs"
        );
    }

    #[tokio::test]
    async fn await_completion_terminalizes_a_lazily_minted_parent() {
        // End-to-end through the real async barrier: a worker that holds the tx
        // until it lazily mints the parent (mirroring the detached GUI conversion)
        // must have its parent terminalized by `await_completion` AFTER the seal —
        // not left Running. This guards the ordering fix (terminate after the
        // coordinator drain, since detached conversions keep a tx clone open).
        let registry = Arc::new(TaskRegistry::new());
        let mut svc = BuildServices::headless();
        svc.task_registry = Some(registry.clone());
        let build_parent_cell = svc.build_parent.clone();
        svc.page_count.store(7, Ordering::Relaxed);
        let barrier = BuildTerminalBarrier::from_services(Some(&svc), Instant::now())
            .expect("services present");
        let svc_arc = Arc::new(svc);
        let handle = BackgroundHandle::spawn_with_pending_and_terminal(
            PendingManifest::new(SiteHashes::default()),
            Some(barrier),
            None,
            |tx, workers| {
                let svc_worker = svc_arc.clone();
                workers.spawn(async move {
                    // Hold tx across the (simulated) conversion, mint the parent
                    // lazily, then drop tx — exactly the detached-conversion shape.
                    let _parent = svc_worker.ensure_build_parent();
                    drop(tx);
                    Ok::<_, BuildError>(())
                });
            },
        );
        let (_sealed, _lease) = handle.await_completion().await.expect("await_completion");
        let parent = build_parent_cell.lock().unwrap().expect("parent minted");
        assert!(
            matches!(build_parent_task(&registry, parent), TaskState::Succeeded { .. }),
            "await_completion must terminalize the lazily-minted parent"
        );
    }

    #[test]
    fn media_and_medialess_receipts_report_the_same_page_count() {
        // FIX 3: the media build's parent receipt Amount must report the SAME page
        // count as the media-less frontend receipt. Both source `html_total`
        // (= documents.len(), the html_pages task total) — `build_inner` stashes it
        // into the shared `page_count` atomic, and the barrier reads it verbatim.
        // A divergent SiteResult.page_count would make the same site show a different
        // "Built · N pages" with vs without media.
        const HTML_TOTAL: usize = 142;
        let (registry, svc, mut barrier) = barrier_fixture();
        // build_inner stashes html_total (NOT the narrower SiteResult.page_count).
        svc.page_count.store(HTML_TOTAL, Ordering::Relaxed);
        let parent = svc.ensure_build_parent().unwrap();
        barrier.terminate_succeeded();
        let task = registry
            .tasks(&WindowId::from("main"), TaskScope::Preview)
            .into_iter()
            .find(|t| t.id == parent)
            .unwrap();
        let amount = task.amount().expect("a successful Build carries an amount");
        // The media receipt's count == html_total == the media-less receipt's count.
        assert_eq!(
            amount.count, HTML_TOTAL as u64,
            "the media parent receipt must report html_total, matching the media-less receipt"
        );
        assert_eq!(amount.noun, "pages");
        assert_eq!(task.verb.as_ref().unwrap().0, "Built");
    }
    // ── The receipt every finished build owes (LOG-C2CF-T0935-08-14) ──────────

    #[tokio::test]
    async fn a_build_with_no_workers_at_all_still_reaches_its_terminal_receipt() {
        // THE regression. A site with no images, no videos and no deferred assets
        // takes `build_inner`'s zero-worker arm, which used to attach no barrier
        // at all — so nothing announced that the build had finished, and the
        // preview sat on whatever it painted before the build (on a first open,
        // the PREVIOUS generation) until the user pressed Cmd+R.
        //
        // Driving the barrier is what emits `BuildComplete`; the parent Job's
        // terminal is the observable proxy for it here, since `emit_tier2` needs
        // an AppHandle that a headless test does not have.
        let (registry, svc, barrier) = barrier_fixture();
        let parent = svc.ensure_build_parent().unwrap();
        let handle = BackgroundHandle::spawn_with_pending_and_terminal(
            PendingManifest::new(SiteHashes::default()),
            Some(barrier),
            None,
            |_tx, _workers| {
                // Zero workers — the media-less build shape.
            },
        );
        handle.await_completion().await.expect("await_completion");
        assert!(
            matches!(build_parent_task(&registry, parent), TaskState::Succeeded { .. }),
            "a build with no media workers must still reach its terminal receipt"
        );
    }

    #[test]
    fn the_receipt_reports_the_counters_the_workers_published() {
        // The worker that knows a number no longer emits the receipt carrying it,
        // so the numbers travel through shared atomics. Pin that wiring: a wrong
        // read here is invisible at runtime (the frontend ignores the counts) and
        // would only ever surface as a wrong CLI summary.
        let (_registry, svc, barrier) = barrier_fixture();
        svc.videos_converted.store(3, Ordering::Relaxed);
        svc.skipped_symlinks.store(2, Ordering::Relaxed);
        match barrier.build_complete_event() {
            crate::build::progress::PipelineEvent::BuildComplete {
                videos_converted,
                skipped_symlinks,
                ..
            } => {
                assert_eq!(videos_converted, 3, "videos_converted comes from the video worker's counter");
                assert_eq!(skipped_symlinks, 2, "skipped_symlinks comes from the assets worker's counter");
            }
            other => panic!("expected BuildComplete, got {other:?}"),
        }
    }

    #[test]
    fn a_superseded_build_owes_no_completion_receipt() {
        // The other half of the invariant: `Drop` cancels the parent and must NOT
        // emit `BuildComplete`. A build that never reached the seal did not
        // finish, and saying otherwise would refresh the preview onto a
        // half-written stage. Asserted structurally — `Drop` calls neither
        // `terminate_succeeded` nor `emit_build_complete`, and the parent lands
        // Cancelled rather than Succeeded.
        let (registry, svc, barrier) = barrier_fixture();
        let parent = svc.ensure_build_parent().unwrap();
        drop(barrier);
        assert!(
            matches!(build_parent_task(&registry, parent), TaskState::Cancelled),
            "a dropped-unsettled barrier cancels rather than completes"
        );
    }
}
