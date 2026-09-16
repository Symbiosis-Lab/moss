//! Build-pipeline service containers — app-runtime side of the M6a boundary.
//!
//! `BuildServices` (the container that couples the pipeline to Tauri),
//! `BackgroundContext` (the post-preview background-work handoff), and
//! `PluginHtmlStore` (the moss-plugin:// protocol store).
//!
//! Entangled residents, kept here deliberately (M5a is a pure move, no
//! redesign): `BuildServices` and `BackgroundContext` hold pure data
//! (`FileInfo`, `SiteHashes`, `MediaMetadata`) alongside
//! runtime fields (cancellation state, the shell ports, singleflight
//! maps, the Job registry), so they live on the runtime side.
//!
//! `BackgroundContext` is the SINGLE owner of everything the deferred phase
//! reads. It used to share that job with a `DeferredWork` struct nested inside
//! it, whose `site_hashes` was a snapshot clone of the render-phase manifest —
//! which meant three separate blocks had to hand-sync entries back into it
//! (slot injection, the fingerprint write, notebook registration) and the
//! pipeline had to fabricate a stub `DeferredWork` after `take()` just to keep
//! `dir_overrides` reachable. The manifest is the coordinator's, the config is
//! this struct's, and neither needs a copy of the other (moss#618).
//!
//! Split out of `types.rs` 2026-08-10 (M5a); `types.rs` re-exports every
//! item at its old path, so consumers are unchanged.

use std::collections::HashMap;

use super::assets::AssetRegistry;
use super::content::{FileInfo, MediaMetadata, SiteHashes};
use super::runtime::{
    ChildProcessRegistry, ImageConversionState, NotebookConversionState, VideoConversionState,
};

/// Context for background video conversion (ADR-001: Two-Phase Build).
///
/// Holds data needed for video conversion tasks that run in the background
/// after the blocking phase completes. This allows the preview to open
/// immediately while videos are still being converted.
///
/// # Usage
/// 1. `generate_blocking_content()` collects video items and returns this context
/// 2. `build()` sends "complete" progress immediately
/// 3. `build()` spawns background task with this context
/// 4. Background task converts videos and emits `asset_ready` events
#[derive(Debug, Clone)]
pub struct BackgroundContext {
    /// Videos that need MOV→MP4 conversion
    pub video_items: Vec<String>,
    /// Images that need WebP variant generation
    pub image_items: Vec<crate::build::image::ImageConversionItem>,
    /// Source folder path (for resolving relative video paths)
    pub source_path: String,
    /// Staging directory where blocking phase builds output.
    /// During rebuilds: site-stage/ (temporary staging)
    /// During first build: site/ (no staging needed)
    pub staging_dir: std::path::PathBuf,
    /// .moss directory for cache paths
    pub moss_dir: std::path::PathBuf,
    /// The previous build's manifest. Read for incremental conversion, and by
    /// the deferred asset walk as the only carry-forward it may re-emit.
    pub previous_hashes: SiteHashes,
    /// Build start time for total_time_ms calculation in BuildComplete event
    pub start_time: std::time::Instant,
    /// Directory path overrides from folder index `url:` frontmatter.
    /// Maps filesystem directory paths to their overridden slugs
    /// (e.g., "视频" → "video", "交互" → "interactive").
    ///
    /// Read by every background output path via `resolve_path_with_overrides()`:
    /// the asset walk, video `.mp4`/`.thumb.jpg` placement, and the image
    /// worker's `.webp` rung paths.
    pub dir_overrides: HashMap<String, String>,
    /// Paths the RENDER phase generated (HTML, CSS, JS, RSS). Two readers, both
    /// in the deferred phase: `remove_stale_html` protects them from the stale
    /// sweep, and the asset walk skips a source `.html` whose path a rendered
    /// page already claims.
    ///
    /// A snapshot, taken by `as_parts_clone` at the end of
    /// `generate_blocking_content` — so it is missing anything registered as
    /// `HashBucket::Files` after that point. Two things are:
    /// `feature_styles::emit` (reached from slot injection) and
    /// `search_lane::adopt_into`. Both readers get away with it only because
    /// they filter to HTML — `remove_stale_html` to `index*.html`, the asset
    /// walk to `html`/`htm` — and neither writes an HTML output. Adding a
    /// post-render `Files` registration that DOES produce HTML means passing
    /// the live `blocking_keys` here instead of a snapshot; the sweep would
    /// otherwise delete the new page.
    pub blocking_keys: std::collections::HashSet<String>,
    /// Site-level meta defaults for bundled-SPA `<head>` injection. Built once
    /// from the homepage frontmatter; consumed by the asset walk after each
    /// source `index.html` is copied to the output. `None` disables injection.
    pub spa_defaults: Option<crate::build::site_meta::spa_inject::SpaDefaultsOwned>,
    /// Passthrough subtree roots threaded from the scan phase. The asset walk
    /// skips SPA meta-injection for these paths.
    pub passthrough_roots: std::collections::HashSet<String>,
    /// Resolved `[site].search` gate
    /// (`layout_config.assets.search` at the end of the blocking phase). The
    /// pipeline caller uses this to build the search index *after*
    /// `remove_stale_html` and with every page already on disk — see
    /// `build/feeds/search.rs` and `docs/reference/search.md`.
    pub search_enabled: bool,
    /// Pre-resolved FFmpeg binary path from scan phase (Task 4: lazy resolution).
    /// When Some, `run_video_conversion` reuses this path instead of calling
    /// `FFmpegManager::get_or_download()` again (which would trigger a second download log).
    pub ffmpeg_bin_path: Option<String>,
    /// Notebook files (.ipynb) that need JupyterLite processing in background.
    /// When non-empty, `run_notebook_processing` downloads JupyterLite assets
    /// (if not cached) and generates viewer HTML pages with JupyterLite iframes.
    pub notebook_files: Vec<FileInfo>,
    /// Responsive-rung collision map: dir-override-mapped path of EVERY
    /// vault image file → that file's ABSOLUTE vault path. Not restricted to
    /// rung-shaped names — a generated rung URL "collides" exactly when it
    /// appears as a key (e.g. a real `photo.w800.webp` beside `photo.jpg`).
    /// Built once in blocking.rs via `build::image::rung_collision_map` and
    /// THREADED here so the background encode worker skips writes to
    /// collided rung paths with the SAME membership test registration used —
    /// the worker must never recompute it (no ProjectStructure there; a
    /// divergent set means a registered-but-never-encoded rung ⇒
    /// sealed-deploy 404, ADR-013).
    pub rung_collisions: std::collections::HashMap<String, std::path::PathBuf>,
}

impl BackgroundContext {
    /// A context with every field empty, for tests that exercise one deferred
    /// worker and care about three or four fields. Use struct-update syntax:
    /// `BackgroundContext { source_path, staging_dir, ..BackgroundContext::for_test() }`.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            video_items: Vec::new(),
            image_items: Vec::new(),
            source_path: String::new(),
            staging_dir: std::path::PathBuf::new(),
            moss_dir: std::path::PathBuf::new(),
            previous_hashes: SiteHashes::default(),
            start_time: std::time::Instant::now(),
            dir_overrides: HashMap::new(),
            blocking_keys: std::collections::HashSet::new(),
            spa_defaults: None,
            passthrough_roots: std::collections::HashSet::new(),
            search_enabled: false,
            ffmpeg_bin_path: None,
            notebook_files: Vec::new(),
            rung_collisions: HashMap::new(),
        }
    }
}

// ============================================================================
// Build Pipeline Service Container
// ============================================================================

/// Build pipeline service container — decouples core build logic from Tauri.
///
/// # Design Principles
///
/// 1. **All paths share one pipeline**: GUI, CLI, and headless (`--no-plugins`)
///    all construct a BuildServices and pass it to the same `build()` function.
///    This ensures CLI thoroughly tests the same code as GUI.
///
/// 2. **Rust core / JS UI separation**: the two fields the pipeline reaches the
///    shell through — `reporter` and `spawner` — are ports whose Tauri-backed
///    implementations live app-side in `crate::events`. No FIELD names an
///    `AppHandle` any more; `from_app` still takes one, because building the
///    ports from a shell is what that constructor is for.
///
/// 3. **Interior mutability**: All fields use atomics or locks, so `&BuildServices`
///    is sufficient for concurrent access. No `&mut` needed.
///
/// # Construction
///
/// - GUI/CLI: `BuildServices::from_app(&app)` — extracts shared state from Tauri
/// - Headless: `BuildServices::headless()` — creates standalone instances
///
/// `Clone` is field-wise `Arc::clone` all the way down (every field is an
/// `Arc`, an `Option<Arc>`, or `Copy`), so a clone shares the SAME counters,
/// cancel tokens and `build_parent` cell — it is a second handle, never a
/// second service set.
#[derive(Clone)]
pub struct BuildServices {
    /// Per-folder session that owns UiBound work counters and the cancel
    /// token. None in headless mode (no Tauri runtime, no folder context).
    /// Non-headless callers always provide a folder path; if no session is
    /// registered yet (e.g. CLI race window), this is also None and the
    /// helper methods become no-ops.
    pub session: Option<std::sync::Arc<crate::system::folder_session::FolderSession>>,
    /// Cancellation token for aborting video conversion on folder switch.
    pub cancellation: std::sync::Arc<VideoConversionState>,
    /// Cancellation token for aborting image conversion.
    ///
    /// Intentionally separate from `cancellation` (video's) — see
    /// `ImageConversionState` docs for the race this isolation prevents.
    pub image_cancellation: std::sync::Arc<ImageConversionState>,
    /// Cancellation token for aborting notebook (JupyterLite) processing.
    ///
    /// Intentionally separate from `cancellation` (video's) — see
    /// `NotebookConversionState` docs for the false-cancel race this
    /// isolation prevents on folder open / app restart.
    pub notebook_cancellation: std::sync::Arc<NotebookConversionState>,
    /// Registry of spawned FFmpeg processes for cleanup on folder switch.
    pub processes: std::sync::Arc<ChildProcessRegistry>,
    /// Where the pipeline says what happened. `crate::events::TauriReporter`
    /// in the shell, `reporter::HeadlessReporter` on a headless build.
    ///
    /// Not an `Option`: absence only ever meant "no GUI", and a reporter that
    /// discards says that without one. What headless still has to say, it says
    /// to the terminal — which is why the two discarding reporters differ on
    /// `is_terminal()` and nothing else.
    pub reporter: std::sync::Arc<dyn crate::build::ports::reporter::BuildReporter>,
    /// Where background media work runs, or `None` when there is no runtime to
    /// spawn onto (CLI, tests) and the dispatchers must run it synchronously so
    /// output is complete when the build returns.
    ///
    /// Set in lockstep with `reporter` — both come from `config.app` — which is
    /// why the old `event_sink.is_none()` served as this predicate. It gave the
    /// right answer while asking the wrong question.
    pub spawner: Option<std::sync::Arc<dyn crate::build::ports::spawner::Spawner>>,
    /// Asset processing status for placeholder support in preview. None in headless.
    pub assets: Option<std::sync::Arc<AssetRegistry>>,
    /// Singleflight dedup for video conversion keyed by source_oid.
    /// When multiple rebuilds try to convert the same video concurrently,
    /// only the first runs FFmpeg — all others block until the first completes
    /// and then receive a clone of its result. The error (if any) is carried
    /// inside VideoConversionOutcome.error so all waiters see the full message.
    pub in_flight_videos: std::sync::Arc<crate::build::cache::Singleflight<crate::build::media::video::VideoConversionOutcome>>,
    /// Singleflight dedup for image conversion keyed by source_oid.
    /// When multiple concurrent rebuilds try to convert the same image to WebP,
    /// only the first runs the encoder — all others block until the first completes
    /// and then receive a clone of its outcome. Generalizes the video-only dedup
    /// pattern to all pipeline tasks per ADR-010.
    pub in_flight_images: std::sync::Arc<crate::build::cache::Singleflight<crate::build::media::image::ImageConversionOutcome>>,
    /// Singleflight dedup for metadata extraction keyed by "meta:{content_hash}".
    /// When multiple concurrent scans (build + plugin hooks) hit a TransformCache miss
    /// for the same file, only the first runs the actual extraction (image dimensions,
    /// dominant color, ffprobe). All others block and receive a clone of the result.
    pub in_flight_metadata: std::sync::Arc<crate::build::cache::Singleflight<MediaMetadata>>,
    /// Accumulated count of symlinks and Finder Aliases that were silently
    /// skipped during the asset copy phase. Written by `copy_deferred_assets`
    /// (background worker) and read by
    /// [`BuildTerminalBarrier`](crate::build::background::BuildTerminalBarrier)
    /// when it emits `BuildComplete`. Both share the same `Arc`; `fetch_add` /
    /// `load` with `Relaxed` ordering is sufficient because they do not
    /// coordinate on any other memory location through this counter.
    ///
    /// Reading it at the post-join barrier rather than inside a peer worker is
    /// also what makes the count *correct*: the write happened-before the join.
    pub skipped_symlinks: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// How many videos this build actually converted. Written by the video
    /// worker at each of its exit points (normal completion, cancellation,
    /// FFmpeg-missing fallback); read once by `BuildTerminalBarrier` for
    /// `BuildComplete.videos_converted`. Same shape and ordering rationale as
    /// `skipped_symlinks`.
    ///
    /// It is a counter rather than a return value because the terminal receipt
    /// is no longer emitted by the worker that knows the number — see the
    /// `BuildTerminalBarrier` type doc for why that separation is deliberate.
    pub videos_converted: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// The unified Job registry (Step 3 Phase 4, R-h). A *separate* channel
    /// from `reporter`'s typed event bus.
    /// The media workers reach the registry through this `Arc` to spawn child
    /// Jobs and drive the parent Build receipt. `None` in headless mode (no
    /// Tauri runtime) — media still dual-emits the legacy `BackgroundProgress`.
    /// `Arc<TaskRegistry>` is `Send + Sync`, so it crosses the `spawn_blocking`
    /// boundary into the media worker fine.
    pub task_registry:
        Option<std::sync::Arc<crate::tasks::TaskRegistry>>,
    /// The parent Build Job's id, lazily minted (Step 3 Phase 4, re-architected
    /// in the Phase-4 review fix). A shared `Arc<Mutex<Option<TaskId>>>` so every
    /// worker clone of `BuildServices` points at the SAME cell. Starts `None`; the
    /// FIRST media worker that does REAL work (a conversion or an advisory) calls
    /// [`ensure_build_parent`](BuildServices::ensure_build_parent), which spawns the
    /// parent ONCE and caches its id here. A content-only rebuild (every media task
    /// cache-skips) never calls `ensure_build_parent`, so the cell stays `None` and
    /// ZERO Jobs are minted (invariant #6 / the heartbeat). The post-join barrier
    /// reads this cell to drive the parent's terminal receipt on EVERY exit path —
    /// images-only, race, or supersede — instead of coupling to the video worker.
    pub build_parent: std::sync::Arc<std::sync::Mutex<Option<crate::tasks::TaskId>>>,
    /// The page count from the render phase (Step 3 Phase 4). The Build Job's
    /// `Amount { count, noun: "pages" }` is sourced HERE: `build_inner` stashes the
    /// render-phase `html_total` (= `documents.len()`, the `html_pages` task total)
    /// into this shared atomic right after the blocking render, and the post-join
    /// barrier reads it when driving the parent receipt. Sourcing `html_total` (not
    /// the narrower `SiteResult.page_count`) keeps the media and media-less receipts
    /// reporting the SAME "Built · N pages". Shared `Arc` across workers like
    /// `skipped_symlinks`; `Relaxed` ordering suffices (no other coordination).
    pub page_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// The deploy plugin's `video_max_size_mb` cap, or `None` for the generic
    /// default.
    ///
    /// Handed in by whoever started the build. The video worker used to read it
    /// itself, which meant `build/media/video.rs` — a leaf of the compiler —
    /// imported plugin discovery AND the manifest schema, both of which ADR-050
    /// §1 keeps out of moss-build by name. `run_pipeline` fills it at the one
    /// site that fills `CacheKeyInputs`, and for the same reason: two callers
    /// that disagree would encode the same video two different ways.
    pub deploy_video_max_size_mb: Option<u32>,
}

impl BuildServices {
    /// Construct a headless instance for a real build of `folder_path`.
    ///
    /// The only difference from [`headless`](Self::headless) is that the
    /// folder's `FolderSession` is attached, which is what gives a headless
    /// build the stage-write lock — in the pipeline, in the seal tail, and in
    /// the watcher's admission probe. Without it a `moss build --serve --watch`
    /// runs rebuild N+1 against the same `staging/` that build N's seal tail is
    /// still materializing, with nothing serializing the two.
    pub fn headless_for(folder_path: &str) -> Self {
        Self {
            session: crate::system::folder_session::registry().get(folder_path),
            ..Self::headless()
        }
    }

    /// Construct a headless instance with no session — unit tests, and any
    /// caller with no folder to key on.
    ///
    /// Creates fresh instances of all services with a discarding reporter and
    /// no spawner. A real headless build wants [`headless_for`](Self::headless_for).
    pub fn headless() -> Self {
        Self {
            session: None,
            cancellation: std::sync::Arc::new(VideoConversionState::default()),
            image_cancellation: std::sync::Arc::new(ImageConversionState::default()),
            notebook_cancellation: std::sync::Arc::new(NotebookConversionState::default()),
            processes: std::sync::Arc::new(ChildProcessRegistry::new()),
            // HeadlessReporter, not NullReporter: both discard, but only this
            // one answers `is_terminal()`, which the pipeline's eprintln sites
            // read. The app-side host seam (`events/host_ports.rs`) replaces
            // this with `CarrierReporter` for a real headless build so Tier-2
            // still reaches the HTTP carrier's SSE stream.
            reporter: std::sync::Arc::new(crate::build::ports::reporter::HeadlessReporter),
            spawner: None,
            // A real registry, not None (2026-08-24, #1097 step 3): the seal
            // tail's moss#867 degrade pass reads `Failed` entries from here,
            // and a `None` made every headless `set_failed` a silent no-op —
            // so a variant that failed to encode shipped as a live 404 inside
            // <picture> from `moss build` while the app degraded it. Fresh
            // per build (the app shares one via `app.manage`), which is right
            // for a process that builds once and exits.
            assets: Some(std::sync::Arc::new(AssetRegistry::new())),
            in_flight_videos: std::sync::Arc::new(crate::build::cache::Singleflight::new()),
            in_flight_images: std::sync::Arc::new(crate::build::cache::Singleflight::new()),
            in_flight_metadata: std::sync::Arc::new(crate::build::cache::Singleflight::new()),
            skipped_symlinks: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            videos_converted: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            // Headless: no Tauri runtime → no Job registry, no parent Build Job.
            // Media still dual-emits the legacy BackgroundProgress.
            task_registry: None,
            build_parent: std::sync::Arc::new(std::sync::Mutex::new(None)),
            page_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            // Filled by `run_pipeline`, which is the one place that knows.
            deploy_video_max_size_mb: None,
        }
    }

    /// Headless, with a live `TaskRegistry` wired in — the media-child-Job
    /// tests' fixture, factored out so more than one test module can build the
    /// same wiring `spawn_media_child_job` expects.
    #[cfg(test)]
    pub(crate) fn with_task_registry() -> (Self, std::sync::Arc<crate::tasks::TaskRegistry>) {
        let registry = std::sync::Arc::new(crate::tasks::TaskRegistry::new());
        let mut svc = Self::headless();
        svc.task_registry = Some(registry.clone());
        (svc, registry)
    }

    /// Idempotent get-or-spawn for the parent Build Job (Step 3 Phase 4, review
    /// fix — FIX 1a). Returns the cached parent `TaskId` if one was already minted
    /// this build, otherwise spawns the parent ONCE (`TaskScope::Preview`,
    /// `TaskKind::Build`, `TaskTone::Ambient`) and caches its id on the shared
    /// `build_parent` cell.
    ///
    /// Called immediately BEFORE minting each media child Job — so the parent is
    /// born only when the first REAL conversion/advisory lands, preserving
    /// parent-before-child ordering. A content-only rebuild never reaches this
    /// (every media task cache-skips), so no parent is minted → zero Jobs
    /// (invariant #6).
    ///
    /// Returns `None` in headless mode (no `task_registry`).
    pub fn ensure_build_parent(&self) -> Option<crate::tasks::TaskId> {
        let registry = self.task_registry.as_ref()?;
        let mut slot = self
            .build_parent
            .lock()
            .expect("BuildServices build_parent mutex poisoned");
        if let Some(id) = *slot {
            return Some(id);
        }
        let id = registry
            .spawn(
                crate::tasks::WindowId::from("main"),
                crate::tasks::TaskScope::Preview,
                crate::tasks::TaskKind::Build,
                crate::tasks::TaskTone::Ambient,
            )
            .id;
        *slot = Some(id);
        Some(id)
    }

    /// Read the parent Build Job's id without spawning one (Step 3 Phase 4, review
    /// fix). Returns `Some` only when a real media child already minted the parent
    /// via [`ensure_build_parent`](Self::ensure_build_parent). The post-join barrier
    /// uses this to decide whether a parent needs terminalizing.
    pub fn current_build_parent(&self) -> Option<crate::tasks::TaskId> {
        *self
            .build_parent
            .lock()
            .expect("BuildServices build_parent mutex poisoned")
    }

    /// Increment the folder session's UiBound counter.
    /// No-op if no session is attached (headless mode).
    pub fn begin_ui_bound(&self) {
        if let Some(s) = self.session.as_ref() {
            s.begin_ui_bound();
        }
    }

    /// Decrement the folder session's UiBound counter (saturating at 0).
    /// No-op if no session is attached (headless mode).
    pub fn end_ui_bound(&self) {
        if let Some(s) = self.session.as_ref() {
            s.end_ui_bound();
        }
    }

    /// Whether the folder session has any UiBound work in flight.
    /// Returns `false` if no session is attached (headless mode).
    pub fn has_ui_bound(&self) -> bool {
        self.session.as_ref().map_or(false, |s| s.has_ui_bound())
    }
}

// ============================================================================
// Plugin HTML Store - For moss-plugin:// protocol
// ============================================================================

/// One stored HTML page: its content, and the plugin that registered it —
/// `None` when the store call came from a webview rather than the trusted
/// engine seam (see [`PluginHtmlStore::store`]).
#[derive(Debug, Clone)]
struct StoredHtml {
    html: String,
    owning_plugin: Option<String>,
}

/// The browser panel's in-flight/committed navigation, for the security
/// property `write_project_file`/`read_project_file`'s caller-identity check
/// depends on: `committed_page_id` must be `None` for the ENTIRE window
/// between issuing a navigation and THAT SAME navigation's commit event —
/// `webview.navigate()` only dispatches, so the OLD page's JS stays alive
/// until the new document actually takes over, and a malicious old page
/// could otherwise spray `write_project_file` claiming a sibling's (public)
/// name and land while the store already named that sibling.
///
/// `expected_url` doubles as the per-navigation correlator: wry's
/// `on_page_load` hook hands back only a URL, no navigation token, so
/// ownership is granted only when a commit's URL still matches the MOST
/// RECENT `begin_browser_panel_navigation` call's. Any OTHER commit —
/// stale (superseded by a later `begin`), or one no `begin` ever announced
/// at all (a link click, a page's own `window.location` self-navigation,
/// `history.back()/forward()`, an external URL) — CLEARS `committed_page_id`
/// to `None` rather than leaving whoever held it before; a mismatch is
/// never treated as "nothing happened", only ever as "no owner". `generation`
/// is the same guarantee restated as a plain counter, kept for tests and
/// diagnostics.
#[derive(Debug, Default, Clone)]
struct BrowserPanelNavigation {
    generation: u64,
    expected_url: Option<String>,
    pending_page_id: Option<String>,
    committed_page_id: Option<String>,
}

/// Storage for plugin HTML content served via moss-plugin:// protocol
///
/// Maps UUIDs to HTML content for plugin UIs. This enables serving dynamic HTML
/// with a valid origin (moss-plugin://local) instead of opaque data URLs,
/// fixing IPC communication issues with Tauri commands.
///
/// # Thread Safety
/// Uses Arc<Mutex<>> for thread-safe access across the async protocol handler
/// and Tauri commands.
#[derive(Debug, Default, Clone)]
pub struct PluginHtmlStore {
    /// Map of UUID -> stored page
    store: std::sync::Arc<std::sync::Mutex<HashMap<String, StoredHtml>>>,
    /// UUID of the `moss-plugin://` HTML page currently shown in the browser
    /// panel (single-slot), or `None`. Set at open/navigate and taken at
    /// teardown so the entry is freed using a tracked id rather than reading
    /// the live `WKWebView.URL` — which `.unwrap()`s inside wry and, under
    /// `panic="abort"`, CRASHES the app when the URL is nil (the browser/login
    /// panel torn down before its first navigation committed). See
    /// `plugins/runtime/browser.rs`.
    ///
    /// This is teardown bookkeeping ONLY — it is set eagerly, before the
    /// navigation it names has committed, so it must never back an
    /// authorization decision. `browser_panel_navigation` below is the
    /// commit-gated field that does.
    current_browser_page: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// The browser panel's navigation state — see [`BrowserPanelNavigation`].
    browser_panel_navigation: std::sync::Arc<std::sync::Mutex<BrowserPanelNavigation>>,
}

impl PluginHtmlStore {
    /// Create a new empty store
    pub fn new() -> Self {
        Self {
            store: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            current_browser_page: std::sync::Arc::new(std::sync::Mutex::new(None)),
            browser_panel_navigation: std::sync::Arc::new(std::sync::Mutex::new(
                BrowserPanelNavigation::default(),
            )),
        }
    }

    /// Store HTML content and return its UUID.
    ///
    /// `owning_plugin` records who this page belongs to, for
    /// `write_project_file`/`read_project_file`'s caller-identity check (the
    /// browser panel showing this page is the "webview URL" side of that
    /// check — see `plugin_config.rs::verify_caller_plugin_identity`). It
    /// must be `None` for a call that came from the webview-facing
    /// `set_action_panel_html` command — only the trusted engine seam
    /// (`AppHost::set_action_panel_html`, sourced from `host.plugin`) may
    /// supply a real id, matching `open_action_panel`'s "no caller id"
    /// precedent: a webview cannot be trusted to name itself.
    pub fn store(&self, html: String, owning_plugin: Option<String>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut map) = self.store.lock() {
            map.insert(id.clone(), StoredHtml { html, owning_plugin });
        }
        id
    }

    /// Retrieve HTML content by UUID
    pub fn get(&self, id: &str) -> Option<String> {
        if let Ok(map) = self.store.lock() {
            map.get(id).map(|entry| entry.html.clone())
        } else {
            None
        }
    }

    /// The plugin that registered the page at `id`, or `None` if the page
    /// has no recorded owner (webview-originated, or the id is unknown) —
    /// the host-derived identity `write_project_file`/`read_project_file`
    /// check a claimed `plugin_name` against.
    pub fn owning_plugin(&self, id: &str) -> Option<String> {
        self.store
            .lock()
            .ok()
            .and_then(|map| map.get(id).and_then(|entry| entry.owning_plugin.clone()))
    }

    /// Remove HTML content by UUID (for cleanup)
    pub fn remove(&self, id: &str) -> Option<String> {
        if let Ok(mut map) = self.store.lock() {
            map.remove(id).map(|entry| entry.html)
        } else {
            None
        }
    }

    /// Record the `moss-plugin://` page id currently shown in the browser
    /// panel (`None` for a non-plugin page, e.g. a Matters/OAuth login URL).
    /// Called on every browser-panel open/navigate.
    pub fn set_current_browser_page(&self, id: Option<String>) {
        if let Ok(mut cur) = self.current_browser_page.lock() {
            *cur = id;
        }
    }

    /// Take (and clear) the tracked browser-panel `moss-plugin://` page id.
    /// Called at teardown to free the entry without reading the live webview
    /// URL. Returns `None` if the panel was showing a non-plugin page.
    pub fn take_current_browser_page(&self) -> Option<String> {
        self.current_browser_page
            .lock()
            .ok()
            .and_then(|mut cur| cur.take())
    }

    /// Call BEFORE issuing the navigation (`webview.navigate()` or building
    /// the webview with this as its initial URL) — never after. Bumps the
    /// generation, records what this navigation is expected to commit as,
    /// and — the load-bearing part — clears `committed_page_id` to `None`
    /// synchronously, so [`committed_browser_panel_owner`] reports no owner
    /// for the entire window until [`commit_browser_panel_navigation`]
    /// proves THIS SAME navigation actually committed. Returns the new
    /// generation (tests only; production code has no navigation token to
    /// hand back to wry).
    pub fn begin_browser_panel_navigation(&self, url: &str, page_id: Option<String>) -> u64 {
        let mut nav = self
            .browser_panel_navigation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        nav.generation += 1;
        nav.expected_url = Some(url.to_string());
        nav.pending_page_id = page_id;
        nav.committed_page_id = None;
        nav.generation
    }

    /// Call from the wry page-load hook once a navigation has PROVABLY
    /// committed — `PageLoadEvent::Started`, which on every desktop backend
    /// wry supports (WKWebView's `didCommitNavigation`, WebView2's
    /// `ContentLoading`, WebKitGTK's `LoadEvent::Committed`) fires only
    /// once the previous document has been superseded, guaranteeing its JS
    /// context is dead. NOT `Finished`, which would leave a needlessly wide
    /// (but not unsafe) "no owner" window after the new page has already
    /// taken over.
    ///
    /// `committed_url` must equal the MOST RECENT
    /// [`begin_browser_panel_navigation`] call's `url` to grant ownership —
    /// any other value CLEARS `committed_page_id` to `None` rather than
    /// leaving the prior owner in place. That covers two distinct cases the
    /// same way on purpose: a stale event for a navigation this panel has
    /// since moved past (superseded by a later `begin` before this one's
    /// commit arrived — the per-navigation generation's reason to exist),
    /// and a commit `begin` never announced at all — a link click, a page's
    /// own `window.location` self-navigation, `history.back()/forward()`
    /// (moss doesn't know the target URL ahead of a back/forward, so it
    /// cannot call `begin` for it), or navigation to an external URL. Only
    /// the first case is truly "ignore, keep the old state"; the second is
    /// "a document loaded that no `begin` vouched for" and must drop
    /// ownership, or the plugin that held it before an unannounced
    /// navigation would go on being trusted for whatever loaded next.
    pub fn commit_browser_panel_navigation(&self, committed_url: &str) {
        let mut nav = self
            .browser_panel_navigation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        nav.committed_page_id = if nav.expected_url.as_deref() == Some(committed_url) {
            nav.pending_page_id.clone()
        } else {
            None
        };
    }

    /// The plugin that owns the browser panel's CURRENTLY COMMITTED page —
    /// `None` for a non-plugin page, for no page at all, AND for the entire
    /// window between a navigation being issued and that exact navigation's
    /// commit event. This is what `write_project_file`/`read_project_file`'s
    /// caller-identity check reads; see [`BrowserPanelNavigation`]'s doc for
    /// why it must never read the eagerly-set `current_browser_page` instead.
    pub fn committed_browser_panel_owner(&self) -> Option<String> {
        let page_id = self
            .browser_panel_navigation
            .lock()
            .ok()
            .and_then(|nav| nav.committed_page_id.clone())?;
        self.owning_plugin(&page_id)
    }

    /// The current navigation generation — tests only, to assert that a
    /// superseded `begin` call really did move the counter past the one a
    /// stale commit event names.
    #[cfg(test)]
    pub(crate) fn current_browser_panel_generation(&self) -> u64 {
        self.browser_panel_navigation
            .lock()
            .map(|nav| nav.generation)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_context_creation() {
        let ctx = BackgroundContext {
            video_items: vec![],
            image_items: Vec::new(),
            source_path: "/test".to_string(),
            staging_dir: std::path::PathBuf::from("/test/.moss/build/staging"),
            moss_dir: std::path::PathBuf::from("/test/.moss"),
            previous_hashes: SiteHashes::default(),
            start_time: std::time::Instant::now(),
            dir_overrides: Default::default(),
            blocking_keys: Default::default(),
            spa_defaults: None,
            passthrough_roots: Default::default(),
            search_enabled: false,
            ffmpeg_bin_path: None,
            notebook_files: vec![],
            rung_collisions: Default::default(),
        };
        assert!(ctx.video_items.is_empty());
        assert_eq!(ctx.source_path, "/test");
    }

    // =========================================
    // BuildServices Tests (TDD - RED phase)
    // =========================================
    //
    // Design Principle: BuildServices decouples the build pipeline from Tauri's AppHandle.
    // All 3 build paths (GUI, CLI, headless) share the same code.
    // CLI can thoroughly test the same Rust core as GUI.

    #[test]
    fn test_build_services_headless_creates_valid_instance() {
        let services = BuildServices::headless();
        // headless has no session attached — UiBound ops are no-ops.
        assert!(services.session.is_none());
        assert!(!services.has_ui_bound());
        // Headless reports to nowhere, but the terminal is its surface.
        assert!(services.reporter.is_terminal());
        assert!(services.spawner.is_none());
        // A registry, not None: headless set_failed must be recorded so the
        // seal tail's degrade pass can see it (#1097 step 3).
        assert!(services.assets.is_some());
    }

    /// `#[tokio::test]`, not `#[test]`: `register_session` reaches the
    /// PROCESS-GLOBAL registry, and its first step drains whatever session is
    /// already there onto `tokio::spawn`. With no prior session that spawn
    /// never runs, which is why a plain `#[test]` passed for as long as this
    /// was the only lib test registering one. `deploy::one_shot`'s seal test now
    /// registers one too, and this began panicking "no reactor running" —
    /// deterministically, on an ordering nothing declares.
    #[tokio::test]
    async fn headless_for_a_registered_folder_attaches_that_session() {
        // The regression this guards: a headless build with no session takes
        // NO stage-write lock — not in the pipeline, not in the seal tail, not
        // in the watcher's admission probe — so `moss build --serve --watch`
        // ran rebuild N+1 against the staging dir build N's seal tail was still
        // materializing. `headless()` (no folder) is the unit-test shape and
        // legitimately has none; a real build goes through `headless_for`.
        let dir = tempfile::tempdir().expect("tempdir");
        let folder = dir.path().to_string_lossy().to_string();

        let unregistered = BuildServices::headless_for(&folder);
        assert!(
            unregistered.session.is_none(),
            "no session registered for this folder yet"
        );

        let session = crate::system::folder_session::register_session(&folder);
        let services = BuildServices::headless_for(&folder);
        let attached = services.session.as_ref().expect("session attached");
        assert!(
            std::sync::Arc::ptr_eq(attached, &session),
            "must attach the SAME session the registry holds, not a fresh one — \
             a private session is a private lock, which is the bug"
        );

        services.begin_ui_bound();
        assert!(services.has_ui_bound(), "counter reaches the real session");
        services.end_ui_bound();
        assert!(!services.has_ui_bound());
    }

    #[test]
    fn test_build_services_headless_tracker_works() {
        // In headless mode there's no session, so begin/end are no-ops and
        // has_ui_bound() always returns false. This documents the contract:
        // callers don't need to special-case headless when calling
        // services.begin_ui_bound() / services.end_ui_bound().
        let services = BuildServices::headless();
        services.begin_ui_bound();
        assert!(!services.has_ui_bound(), "headless: begin is a no-op");
        services.end_ui_bound();
        assert!(!services.has_ui_bound());
    }

    #[test]
    fn test_build_services_headless_conversion_id_increments() {
        // Pre-Track A this test covered cancel-flag round-trip on
        // `services.cancellation`. The flag is gone (cancellation lives on
        // FolderSession); this test now only covers the surviving behavior:
        // `start_new_conversion()` returns a positive, monotonic id.
        let services = BuildServices::headless();
        let id1 = services.cancellation.start_new_conversion();
        let id2 = services.cancellation.start_new_conversion();
        assert!(id1 > 0);
        assert!(id2 > id1);
    }

    // =========================================
    // PluginHtmlStore Tests (relocated from plugin_protocol_tests.rs)
    // =========================================

    #[test]
    fn test_plugin_html_store_and_retrieve() {
        let store = PluginHtmlStore::new();
        let html = "<html><body>Test</body></html>".to_string();
        let id = store.store(html.clone(), None);
        let retrieved = store.get(&id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), html);
    }

    #[test]
    fn test_plugin_html_store_multiple() {
        let store = PluginHtmlStore::new();
        let html1 = "<html><body>First</body></html>".to_string();
        let html2 = "<html><body>Second</body></html>".to_string();
        let id1 = store.store(html1.clone(), None);
        let id2 = store.store(html2.clone(), None);
        assert_eq!(store.get(&id1).unwrap(), html1);
        assert_eq!(store.get(&id2).unwrap(), html2);
    }

    #[test]
    fn test_plugin_html_store_remove() {
        let store = PluginHtmlStore::new();
        let html = "<html><body>Test</body></html>".to_string();
        let id = store.store(html.clone(), None);
        assert!(store.get(&id).is_some());
        let removed = store.remove(&id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), html);
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_plugin_html_store_browser_page_tracking() {
        // The browser-panel teardown frees a moss-plugin:// entry by this
        // tracked id instead of reading the live (possibly-nil) webview URL.
        let store = PluginHtmlStore::new();
        let id = store.store("<html></html>".to_string(), None);

        store.set_current_browser_page(Some(id.clone()));
        // take returns the tracked id, then clears it.
        assert_eq!(store.take_current_browser_page(), Some(id.clone()));
        assert_eq!(store.take_current_browser_page(), None, "take must clear");

        // A non-plugin page (login URL) tracks None — nothing to free.
        store.set_current_browser_page(Some(id.clone()));
        store.set_current_browser_page(None);
        assert_eq!(store.take_current_browser_page(), None);
    }

    /// Only a store call carrying a real `owning_plugin` (the trusted engine
    /// seam) records an owner — the webview-facing path always passes
    /// `None`, and an unknown id has no owner at all.
    #[test]
    fn test_plugin_html_store_owning_plugin() {
        let store = PluginHtmlStore::new();
        let owned_id = store.store("<html></html>".to_string(), Some("matters".to_string()));
        let unowned_id = store.store("<html></html>".to_string(), None);

        assert_eq!(store.owning_plugin(&owned_id), Some("matters".to_string()));
        assert_eq!(store.owning_plugin(&unowned_id), None);
        assert_eq!(store.owning_plugin("not-a-real-id"), None);
    }

    // =========================================
    // Browser-panel navigation gating (the b/navigate-in-place race)
    // =========================================

    /// The load-bearing property: from the moment a navigation is issued
    /// until THAT SAME navigation's commit event, no page id is claimed as
    /// owner — a caller cannot land during the in-flight window no matter
    /// what it names, because `committed_browser_panel_owner` reports
    /// `None` regardless of who claims to be it.
    #[test]
    fn begin_clears_the_owner_for_the_in_flight_window() {
        let store = PluginHtmlStore::new();
        let owned_id = store.store("<html></html>".to_string(), Some("matters".to_string()));

        // A prior navigation already committed, naming matters as owner.
        store.begin_browser_panel_navigation("moss-plugin://local/prev", Some(owned_id.clone()));
        store.commit_browser_panel_navigation("moss-plugin://local/prev");
        assert_eq!(store.committed_browser_panel_owner(), Some("matters".to_string()));

        // Issuing the NEXT navigation clears the owner immediately — before
        // any commit event for it can possibly have fired.
        store.begin_browser_panel_navigation("moss-plugin://local/next", Some(owned_id.clone()));
        assert_eq!(
            store.committed_browser_panel_owner(),
            None,
            "no claim may be honored while the old page's JS could still be alive"
        );
    }

    /// A commit event for a navigation this panel has since moved past
    /// (its URL no longer matches what the CURRENT generation expects) is
    /// ignored — a late/stale event cannot retroactively set the owner.
    #[test]
    fn a_stale_commit_event_for_a_superseded_navigation_is_ignored() {
        let store = PluginHtmlStore::new();
        let id_a = store.store("<html></html>".to_string(), Some("matters".to_string()));
        let id_b = store.store("<html></html>".to_string(), Some("github".to_string()));

        let gen_a = store.begin_browser_panel_navigation("moss-plugin://local/a", Some(id_a));
        // Superseded before A's commit event ever arrived.
        let gen_b = store.begin_browser_panel_navigation("moss-plugin://local/b", Some(id_b));
        assert!(gen_b > gen_a, "generation must advance per navigation");

        // A's late commit event finally arrives — its URL no longer matches
        // what the panel is currently on, so it must not set the owner.
        store.commit_browser_panel_navigation("moss-plugin://local/a");
        assert_eq!(
            store.committed_browser_panel_owner(),
            None,
            "a stale commit for a superseded navigation must not set the owner"
        );
        assert_eq!(store.current_browser_panel_generation(), gen_b);

        // B's own (matching) commit event DOES set the owner.
        store.commit_browser_panel_navigation("moss-plugin://local/b");
        assert_eq!(store.committed_browser_panel_owner(), Some("github".to_string()));
    }

    /// The matching case end to end: owner is set only after the load event
    /// for the CURRENT navigation, and reads back correctly.
    #[test]
    fn owner_set_after_the_matching_load_event() {
        let store = PluginHtmlStore::new();
        let id = store.store("<html></html>".to_string(), Some("matters".to_string()));

        store.begin_browser_panel_navigation("moss-plugin://local/x", Some(id));
        assert_eq!(store.committed_browser_panel_owner(), None, "not yet committed");

        store.commit_browser_panel_navigation("moss-plugin://local/x");
        assert_eq!(store.committed_browser_panel_owner(), Some("matters".to_string()));
    }

    /// A page that owns the panel can navigate itself away without moss ever
    /// calling `begin` for the destination — a link click or a
    /// `window.location` self-navigation. The resulting commit event's URL
    /// does not match the (still-set-from-A) `expected_url`, and must CLEAR
    /// the owner, not leave A trusted for whatever loaded next.
    #[test]
    fn a_self_navigation_commit_to_a_different_url_clears_the_prior_owner() {
        let store = PluginHtmlStore::new();
        let id_a = store.store("<html></html>".to_string(), Some("matters".to_string()));

        store.begin_browser_panel_navigation("moss-plugin://local/a", Some(id_a));
        store.commit_browser_panel_navigation("moss-plugin://local/a");
        assert_eq!(store.committed_browser_panel_owner(), Some("matters".to_string()));

        // A's own page navigates itself (link click / `window.location`) —
        // no `begin` was ever called for this URL.
        store.commit_browser_panel_navigation("https://attacker.example/payload");
        assert_eq!(
            store.committed_browser_panel_owner(),
            None,
            "an unannounced commit must drop the prior owner, not inherit it"
        );
    }

    /// `browser_chrome_action("back")` evals `history.back()` without
    /// knowing the destination URL, so it cannot call `begin` for it. The
    /// resulting commit must not leave the panel attributed to whichever
    /// plugin owned it before back/forward navigated away.
    #[test]
    fn a_back_forward_commit_with_no_announced_target_clears_the_owner() {
        let store = PluginHtmlStore::new();
        let id_a = store.store("<html></html>".to_string(), Some("matters".to_string()));

        store.begin_browser_panel_navigation("moss-plugin://local/a", Some(id_a));
        store.commit_browser_panel_navigation("moss-plugin://local/a");
        assert_eq!(store.committed_browser_panel_owner(), Some("matters".to_string()));

        // history.back() lands on whatever the webview's history holds —
        // moss never announced this URL via `begin`.
        store.commit_browser_panel_navigation("moss-plugin://local/previous-in-history");
        assert_eq!(
            store.committed_browser_panel_owner(),
            None,
            "a back/forward commit with no matching begin must clear the owner"
        );
    }

    /// Navigating to an external (non-plugin) URL — e.g. a Matters/OAuth
    /// login page — must also clear the owner, not preserve the plugin that
    /// held the panel beforehand.
    #[test]
    fn a_commit_to_an_external_url_clears_the_owner() {
        let store = PluginHtmlStore::new();
        let id_a = store.store("<html></html>".to_string(), Some("matters".to_string()));

        store.begin_browser_panel_navigation("moss-plugin://local/a", Some(id_a));
        store.commit_browser_panel_navigation("moss-plugin://local/a");
        assert_eq!(store.committed_browser_panel_owner(), Some("matters".to_string()));

        store.commit_browser_panel_navigation("https://matters.town/oauth/authorize");
        assert_eq!(
            store.committed_browser_panel_owner(),
            None,
            "a commit to an external URL must clear the owner"
        );
    }
}
