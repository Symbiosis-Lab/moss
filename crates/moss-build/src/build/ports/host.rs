//! Everything the pipeline needs from whichever host is running it.
//!
//! Until 2026-08-27 these capabilities were fetched from
//! `PipelineConfig.app: Option<tauri::AppHandle>` via managed-state lookups
//! scattered through `run_pipeline` — which is why the conductor could not
//! leave the app crate (M6a). Now the host resolves them ONCE at the entry
//! point: the app side constructs [`HostPorts`] via `crate::events::host_ports`
//! (capturing its `AppHandle`), headless callers get the same shape with the
//! headless arms. The conductor consumes; it never asks "do I have an app".
//!
//! ## Why this lives here and not beside `run_pipeline`
//!
//! It was declared in `build.rs` until 2026-08-27, "deliberately outside ratchet
//! row (o) like `PipelineConfig` itself". That reasoning does not hold:
//! `PipelineConfig` is what a *caller* asks the build to do, while `HostPorts`
//! **is the seam** — the exact thing NORTH-STAR's abort threshold is defined
//! over. Row (o) scans `ports.rs` + `ports/**`, so declaring the widest seam
//! struct one directory up meant the threshold could not fire no matter how
//! wide it got, and it had reached sixteen direct members before anyone counted.
//! Moving the declaration here is what makes the falsifier able to fail.
//!
//! The row-(o) jump this caused is newly *measured* surface, not newly *added*
//! surface, and is accepted under that name.

use std::sync::Arc;

use crate::vault::paths::VaultRoot;

/// The future a [`HostPorts::launch_server`] closure returns.
pub type ServerFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<u16, String>> + Send>>;

/// The future a [`HostPorts::server_diff`] closure returns: the hosting
/// server's own `(need, remove)` counts for a sealed manifest, or `None`
/// when the host cannot or will not ask (not moss-hosted, no identity,
/// network failure). `None` is indistinguishable from "no such surface" on
/// purpose — the consumer degrades to a blank either way, never to an error.
pub type ServerDiffFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Option<(usize, usize)>> + Send>>;

/// Ask the hosting server what publishing this manifest would transfer:
/// `(files map, generation id)` in, [`ServerDiffFuture`] out. The map is the
/// sealed manifest's `files()` — the exact wire manifest deploy itself sends
/// to the sync endpoint, so the counts are the server's own answer. With the
/// endpoint's caveat: a partial upload of this exact manifest staged on the
/// server shrinks `need` to the remainder (resume), so the counts can briefly
/// under-report a transfer that is already half done.
pub type ServerDiff = Arc<
    dyn Fn(std::collections::HashMap<String, String>, String) -> ServerDiffFuture + Send + Sync,
>;

/// The host's own records and the two user-owned files only the host may write.
///
/// These six were six separate `Arc<dyn Fn…>` fields on [`HostPorts`] until
/// 2026-08-27, each with its own signature and two of them wrapped in an
/// `Option` that every consumer had to unwrap. They are one concept — *ask the
/// host about its bookkeeping, or hand it back a fact the build just learned* —
/// so they are one trait, matching the three ports that were already traits.
///
/// The fold is not a way to shrink the seam: ratchet row (o) counts a trait
/// method exactly as it counts a struct member, which is the abort-threshold rule's whole point.
/// What it buys is that the shapes are named and the `None` arms are a real
/// implementation rather than a silent skip at each call site.
pub trait HostStore: Send + Sync {
    /// Is this generation pinned (a deploy is in flight)? Headless: never.
    fn is_pinned(&self, generation: &str) -> bool;

    /// Bring the vault's on-disk `.moss/` up to date before the build reads it
    /// — the `config.toml` schema migration and the legacy email-path rename.
    ///
    /// Both rewrite files the user owns, so they stay host territory.
    /// They are a PRECONDITION of reading the vault, which is why
    /// the build asks for them rather than each caller remembering: "the only
    /// migration point both the GUI and the CLI reach" was previously a
    /// comment, and a comment is not a guarantee once moss-cli is its own
    /// binary. A port makes it one.
    ///
    /// Soft-failing by contract: the implementation logs and continues, because
    /// a corrupt config must not take down the whole build.
    fn run_vault_migrations(&self, root: &VaultRoot);
}

/// Everything the pipeline needs from whichever host is running it.
///
/// `Option` fields mean "this host has no such surface" and each `None` arm
/// preserves the exact pre-port headless behavior at its consumer.
pub struct HostPorts {
    /// A shell (webviews, managed state) is attached. Used only where the
    /// pre-port code branched on `app.is_some()` for something that is not a
    /// capability below (the long-lived-process predicate, the generator
    /// seal gate).
    pub shell_attached: bool,
    /// The app-managed serve-dir cell (`SiteDirectoryState.current_dir`), so
    /// the build drives the SAME Arc the preview server reads (C3 /
    /// regression cce38aa0fc). `None` → the build mints a fresh cell and
    /// hands it to `launch_server`.
    pub site_dir: Option<Arc<std::sync::RwLock<std::path::PathBuf>>>,
    /// Shared media-metadata dedup cache (app-managed). `None` → scans
    /// dedup nothing, exactly as before.
    pub metadata_dedup:
        Option<Arc<crate::build::cache::Singleflight<crate::types::content::MediaMetadata>>>,
    /// Where initial-scan progress events go. `None` → silent scan.
    pub scan_events: Option<crate::build::scan::scan::ScanEventEmitter>,
    /// The tier-2 (typed-bus / SSE) reporter — `crate::events::tier2_reporter`'s
    /// answer, resolved once. Distinct from `PipelineConfig.progress`, which is
    /// the tier-1 loading-screen/CLI sink.
    pub events: crate::build::ProgressSink,
    /// Who is told when a generation seals.
    pub announcer: Arc<dyn super::announcer::SealAnnouncer>,
    /// The host's one async runtime, as a port. Used for the detached
    /// seal+persist task and the fire-and-forget native/process spawns.
    pub spawner: Arc<dyn super::spawner::Spawner>,
    /// See [`HostStore`].
    pub store: Arc<dyn HostStore>,
    /// The host's plugin managers, one per project. The app hands in the
    /// cache every command shares; a headless host builds a cache of its own
    /// over the carrier reporter. Whether plugins run at all is a
    /// `PluginMode` decision in the config, never a fork on which host this
    /// is — the `PluginRuntime` trait that used to sit here retired on
    /// 2026-09-05 when the manager crossed into this crate.
    pub plugins: Arc<crate::plugins::manager::ManagerCache>,
    /// See [`ServerDiff`]. `None` (headless, or a shell with no hosted site)
    /// means the last-resort change-set arm in `manifest::backfill::for_seal`
    /// is skipped and the surfaces stay honestly blank.
    pub server_diff: Option<ServerDiff>,
    /// Start (or reuse) the preview server for `moss_dir`, sharing the given
    /// serve-dir cell. The server is host territory (its core relocates there
    /// later); the pipeline only asks for a port.
    pub launch_server: Option<
        Arc<
            dyn Fn(String, Option<Arc<std::sync::RwLock<std::path::PathBuf>>>) -> ServerFuture
                + Send
                + Sync,
        >,
    >,
    /// The per-build service bundle, constructed by the host
    /// (`BuildServices::from_app` / `headless_for`).
    pub services: crate::types::services::BuildServices,
}

/// A host with no capabilities at all — every `Option` is `None` and every
/// trait answer is the neutral one. For unit tests of pure `PipelineConfig`
/// logic (the incremental gates); a test that RUNS the pipeline wants the
/// app crate's `host_ports`, which is why this never leaves `cfg(test)`.
#[cfg(test)]
pub(crate) fn test_host_ports() -> HostPorts {
    struct NoStore;
    impl HostStore for NoStore {
        fn is_pinned(&self, _generation: &str) -> bool {
            false
        }
        fn run_vault_migrations(&self, _root: &VaultRoot) {}
    }
    struct NoSpawner;
    impl super::spawner::Spawner for NoSpawner {
        fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
            task();
        }
        fn spawn(&self, task: super::spawner::Task) -> super::spawner::Joining {
            Box::pin(async move {
                task.await;
                Ok(())
            })
        }
    }
    HostPorts {
        shell_attached: false,
        site_dir: None,
        metadata_dedup: None,
        scan_events: None,
        events: crate::build::null_sink(),
        announcer: Arc::new(super::announcer::LogAnnouncer),
        spawner: Arc::new(NoSpawner),
        store: Arc::new(NoStore),
        plugins: Arc::new(crate::plugins::manager::ManagerCache::headless(crate::build::null_sink())),
        server_diff: None,
        launch_server: None,
        services: crate::types::services::BuildServices::headless(),
    }
}
