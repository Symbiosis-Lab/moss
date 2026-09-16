//! Frontend-facing event payloads.
//!
//! Serializable structs moss emits to the app's webviews (build progress,
//! scan stages, file-watch deltas) plus `SystemInfo` for support and
//! diagnostics. Data-only, but app-shaped: a headless moss-build crate has
//! no frontend to emit these to, so they sit on the app side of the M6a
//! boundary.
//!
//! Split out of `types.rs` 2026-08-10 (M5a); `types.rs` re-exports every
//! item at its old path, so consumers are unchanged.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Real-time progress update for build process.
///
/// Provides structured progress information for the frontend
/// to display during website build and server startup.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct ProgressUpdate {
    /// Current step being executed
    pub step: String,
    /// Detailed message about current operation
    pub message: String,
    /// Progress completion percentage (0-100)
    pub percentage: u8,
    /// Whether this step is completed
    pub completed: bool,
    /// Preview server port when server is ready (optional)
    pub port: Option<u16>,
    /// Whether the site is empty (no content files found)
    pub is_empty: Option<bool>,
}

/// System diagnostic information for debugging and user support.
///
/// Contains runtime information about the application's integration
/// with the operating system and current operational status.
/// Used by support commands and debugging workflows.
#[derive(Serialize, Deserialize, Debug, Type)]
pub struct SystemInfo {
    /// Operating system identifier (e.g., "macos", "windows", "linux")
    pub os: String,
    /// Application version string from Cargo.toml
    pub app_version: String,
}

/// File change event for live development mode.
///
/// Provides information about file system changes during file watching
/// to enable smart frontend responses (refresh vs redirect vs skip).
///
/// Two consumer paths, each reading a disjoint subset of fields:
///
/// **Output-domain** (consumed by `preview-actions.ts`):
/// - `changed_output_files`: refresh decisions
/// - `deleted_paths`: triggers redirect-home when current page is deleted
/// - `moved_output_paths`: triggers URL update when current page's output moved
///
/// **Source-domain** (consumed by `EntryRegistry` in the action panel — added
/// by the entry-id architecture, see `docs/archive/2026-05-25-entry-id-architecture.md`):
/// - `source_creates`: new source files appearing this rebuild
/// - `source_deletes`: source files removed this rebuild
/// - `source_renames`: `(old_source_path, new_source_path)` pairs for the
///   same file moving to a new path (inode-paired by the watcher)
/// - `modified_paths`: source files modified in place (logging only)
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct FileChangeEvent {
    /// Output files whose content hash changed (relative to the staging dir).
    /// This is the definitive list - frontend refreshes only if current page is here.
    pub changed_output_files: Option<Vec<String>>,
    /// Output paths (relative to the staging dir) that were deleted in
    /// the rebuild. Frontend triggers redirect-home when the current iframe
    /// path matches one of these. Same domain as `changed_output_files` and
    /// `moved_output_paths` — see `pathMatchesCurrentPage` in
    /// `frontend/app/preview/path-matcher.ts` for the matching rules.
    pub deleted_paths: Option<Vec<String>>,
    /// `(old_output_path, new_output_path)` pairs for any iframe target that
    /// should follow rather than redirect-home. Two producers feed in here:
    /// (1) filesystem-level renames stitched by `notify-debouncer-full` from
    /// inode-paired Modify(Name(From/To)) events, and (2) source-stable URL
    /// changes where the source filename is unchanged but its slug moved
    /// (e.g., the user removed frontmatter `title:`). Same output domain as
    /// `deleted_paths`. Frontend navigates the iframe to the new URL when
    /// the current path matches an old path. Populated by
    /// `build_rebuild_event_with_renames` in `watch.rs`.
    pub moved_output_paths: Option<Vec<(String, String)>>,
    /// **Source-domain.** New source files appearing this rebuild (project-
    /// root-relative paths, forward-slash separators). Deduped against
    /// `source_renames` — a path that's the NEW side of a rename does not
    /// appear here. Consumed by `EntryRegistry.upsert` to mint EntryIds for
    /// newly-visible files. See `docs/archive/2026-05-25-entry-id-architecture.md`.
    pub source_creates: Option<Vec<String>>,
    /// **Source-domain.** Source files removed this rebuild. Deduped against
    /// `source_renames` — a path that's the OLD side of a rename does not
    /// appear here. Consumed by `EntryRegistry.delete` to retire EntryIds.
    pub source_deletes: Option<Vec<String>>,
    /// **Source-domain.** `(old_source_path, new_source_path)` pairs for FS
    /// renames stitched by `notify-debouncer-full`. Consumed by
    /// `EntryRegistry.rename` to preserve EntryId across rename. Note this
    /// is the SOURCE-domain analogue of `moved_output_paths`; the same FS
    /// rename event populates both fields (the registry consumes source
    /// pairs; the preview consumes the resolved output pairs).
    pub source_renames: Option<Vec<(String, String)>>,
    /// Files that were modified (source paths) - for logging only, not used for refresh
    pub modified_paths: Option<Vec<String>>,
}

impl FileChangeEvent {
    /// Create a new empty file change event
    pub fn new() -> Self {
        Self {
            changed_output_files: None,
            deleted_paths: None,
            moved_output_paths: None,
            source_creates: None,
            source_deletes: None,
            source_renames: None,
            modified_paths: None,
        }
    }

    /// Set the changed output files
    pub fn set_changed_output_files(&mut self, files: Vec<String>) {
        if files.is_empty() {
            self.changed_output_files = None;
        } else {
            self.changed_output_files = Some(files);
        }
    }
}

/// Emitted periodically (throttled ~10 Hz) while `scan_folder_with_dedup`
/// walks the project directory. Lets the frontend show a "Scanning files…"
/// stage with a running count before any totals are known.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct ScanProgress {
    /// Files found so far (including evicted cloud files).
    pub found: usize,
    /// Evicted (dataless cloud-provider) files found so far.
    pub evicted: usize,
}

/// Emitted exactly once, when the directory walk finishes. The frontend
/// uses `evicted_count` to decide whether the next stage is
/// "Downloading from {provider}…" (materialization) or
/// "Generating pages…" (build).
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct ScanComplete {
    /// Total files found in the walk.
    pub total_files: usize,
    /// How many of those were evicted (dataless).
    pub evicted_count: usize,
    /// Display name of the detected cloud provider, if any evicted
    /// files were found and the path lives under a known File Provider
    /// root. `None` means the next stage is build, not materialization.
    ///
    /// TODO: switch to sending the enum key (`"icloud"`, `"dropbox"`, …) and
    /// localise the display string on the frontend. Part of the typed-event
    /// migration tracked in issue #523.
    pub cloud_provider: Option<String>,
}


// ─────────────────────────────────────────────────────────────────────────────
// The typed moss→frontend event bus — crossed from `src-tauri/src/events.rs`
// at S1 of the ADR-067 relocation (2026-08-28, plan decision 4): the SSE event
// carrier (`crate::ops::serve::events`) publishes `MossEvent`s, so the contract
// TYPES live crate-side while the Tauri emitters (`emit_moss_event*`) stay
// app-side. Desktop-only variants (toast, fullscreen, panel, updater) ride
// along deliberately — one contract, two envelopes; a browser client ignoring
// `FullscreenChanged` is cheaper than two event vocabularies drifting.
// Routing table, legacy channels, and the emit helpers: `src-tauri/src/events.rs`.
// ─────────────────────────────────────────────────────────────────────────────

/// Re-export domain payload types so the enum stays a thin tag.
pub use crate::build::manifest::change_set::ChangeSet;
pub use crate::build::progress::{
    AssetReady, AssetsSettled, BackgroundProgress, BuildComplete, SettledAsset, SitePromoted,
    VideoConversionProgress,
};
pub use crate::plugins::types::PluginProgressEvent;
pub use crate::tasks::PanelTaskWire;
pub use crate::types::toast::ToastPayload;

/// Tauri channel name for the typed bus (the SSE carrier frames events under
/// the same name). Frontend mirror lives at `frontend/app/utils/moss-events.ts`.
pub const MOSS_EVENT_CHANNEL: &str = "moss-event";

// The S1-crossed payload vocabulary lives in the sibling
// `types/event_payloads.rs` (row-(a) length budget); re-exported here so the
// enum stays a thin tag and every consumer spells `types::events::X`.
pub use crate::types::event_payloads::{
    host_from_url, ActionPanelOpened, CredentialRequest, LoginFailureReason, NestedRootInfo,
    NestedRootsReport, NetworkFailureClass, PageVerdict, PublishReceipt, PublishReceiptDomain,
    PublishReceiptLive, PublishReceiptNewsletter, PublishReceiptPage, PublishReceiptPageKind,
    PublishReceiptUploaded, PublishTarget, PublishVerdict, PublishVerdictState, RootClass,
    SuggestedSite, ThresholdPrompt, ThresholdPromptKind, UpdateAvailable, UpdateCheckResult,
    UpdateDownloadProgress,
};

#[derive(Clone, Debug, Serialize, Type)]
#[serde(tag = "kind", content = "payload")]
pub enum MossEvent {
    BackgroundProgress(BackgroundProgress),
    AssetReady(AssetReady),
    /// Post-seal sweep of materialized image/video variants (page-agnostic
    /// srcset swap relay). See `crate::build::progress::AssetsSettled`.
    AssetsSettled(AssetsSettled),
    VideoConversionProgress(VideoConversionProgress),
    BuildComplete(BuildComplete),
    /// `current` now points at this generation. `BuildComplete` says the build
    /// finished; this says the result is what a preview would load. They are
    /// not the same instant, and the gap between them is where a UI can
    /// truthfully-but-wrongly report a change as live — see `SitePromoted`.
    SitePromoted(SitePromoted),
    PluginProgress(PluginProgressEvent),
    FileChanged(FileChangeEvent),
    DeployWaitingForBuild,
    /// What publishing right now would change on the live site, recomputed at
    /// every seal and once more when a deploy discovers there is no local
    /// record to classify against.
    ///
    /// On the bus rather than a `Channel<T>` (ownership row 21's chooser)
    /// because it has several independent subscribers — the publish button's
    /// composition ring, its tooltip, the progress panel, and later the globe
    /// badge — which are three renderings of ONE change set. It is also a
    /// resting, low-frequency value, not a progress stream: `deploy-progress`
    /// stays off the bus for exactly the opposite reason.
    PublishChangeSet(ChangeSet),
    /// Preview server could not bind / verify ready. The build pipeline still
    /// completes (with `port: None` in the eventual `ProgressUpdate`), but
    /// without this signal the frontend's `preview-state.ts` silent-warns on
    /// the portless completion and leaves the panel stuck at the last
    /// `BackgroundProgress` value. This event surfaces the real failure so
    /// the user sees an error instead of a hung progress bar.
    PreviewServerFailed { message: String },
    ActionPanelLoaded,
    ActionPanelOpened(ActionPanelOpened),
    ActionPanelClosed,
    DividerPositionChanged { position: f64 },
    /// Preview-tiling mode flip (wry#175 cursor constraint — see
    /// system/preview_tiling.rs). `tiled: true` → the preview webview is
    /// natively tiled beside the panel; the shell zeroes its
    /// `--action-panel-width` inset (the webview origin replaces it).
    /// `tiled: false` → full-window webview; `inset` carries the px the
    /// shell should write (the live panel width during a divider gesture,
    /// 0 when no panel is visible). divider.ts `applyPreviewTiling` is the
    /// frontend sink.
    PreviewTiling { tiled: bool, inset: f64 },
    OpenSettingsModal,
    FullscreenChanged { is_fullscreen: bool },
    /// Fires at the START of a native fullscreen ENTER/EXIT transition (macOS
    /// NSWindowWillEnter/ExitFullScreen), before macOS reveals/hides the traffic
    /// lights — so the frontend can move chrome (the corner Edit button) first.
    /// `FullscreenChanged` (emitted later, on Resized) remains the backstop.
    FullscreenWillChange { will_be_fullscreen: bool },
    ShowToast(ToastPayload),
    /// Mutate an existing toast in place. `id` (in payload) selects which.
    ShowToastUpdate(ToastPayload),
    UpdateAvailable(UpdateAvailable),
    UpdateCheckResult(UpdateCheckResult),
    /// Byte progress of the installer download driven by `install_pending_update`.
    /// Feeds the same update toast the "Update & Restart" button turns into a
    /// progress bar, so the download is visible while it runs instead of only
    /// when it ends.
    UpdateDownloadProgress(UpdateDownloadProgress),
    /// `PanelTask` lifecycle event (ADR-015). Emitted by `TaskRegistry`
    /// after every spawn / progress / awaiting / terminal transition.
    /// Frontend renderers (`breadcrumb-hairline.ts`, the progress panel,
    /// future Inline/Narrated/Awaiting renderers) filter on `payload.tone`
    /// and `payload.scope` to decide whether to react.
    ///
    /// Added in T2 (2026-05-28) — T1 deferred the wire bridge because
    /// `PanelTask` contains `Instant`; `PanelTaskWire` strips it. See the
    /// module docstring in `tasks.rs` for the full rationale.
    PanelTaskUpdate(PanelTaskWire),
    /// A source image file was created, modified (in-place), deleted, or
    /// renamed on the filesystem. Emitted by the file watcher BEFORE the
    /// rebuild-eligibility gate — fires even if the change does not trigger a
    /// rebuild (e.g. the content-hash gate suppresses it). The editor's
    /// reference resolver uses this to bump the per-asset cache-bust token so
    /// inline image widgets repaint with fresh source bytes, and to clear its
    /// resolved cache so a deleted/renamed source re-resolves to not-found.
    ///
    /// `path` is project-root-relative with a leading `/`
    /// (e.g. `/图片/摄影/X.jpeg`), matching the `request_url` the editor uses
    /// for its `moss-source://localhost<request_url>?_t=<token>` image URLs.
    SourceAssetChanged {
        /// Project-root-relative path with a leading `/`, e.g. `/assets/photo.jpg`.
        path: String,
    },
    /// A file was created on disk via a RAW FS event (outside a process-hook build).
    /// Emitted before the rebuild-eligibility gate so the editor can flash the deepest
    /// visible ancestor folder even when the file doesn't trigger a rebuild.
    /// `paths` are project-root-relative, forward-slash, NO leading slash (the
    /// `path_to_relative_key` domain — NOT the leading-slash moss-source:// domain).
    RawFileCreated { paths: Vec<String> },
    /// Onboarding T9: ask the app shell (main webview) to open the plugin
    /// catalog filtered to importers. Produced by the editor webview's
    /// "Plugin" card; consumed by `app.ts`'s `onMossEvent("OpenPluginsView", …)`
    /// listener which opens the settings modal at the `channels` section.
    ///
    /// **Routed via `emit_to("main", …)` — NOT broadcast.** Tauri webviews
    /// are isolated `Window` instances; a `window.dispatchEvent` from the
    /// editor webview never reaches the main webview's listener, so the
    /// previous T9 implementation (a `CustomEvent`) silently no-op'd.
    /// `app.emit_to("main", …)` is the supported cross-webview path.
    /// See project memory: "NEVER use `app.emit()` broadcast".
    OpenPluginsView {
        /// Capability the catalog pre-filters to. The catalog recognizes
        /// `"import"` (onboarding "Connect a service" card) and `"deploy"`
        /// (deploy-target chooser); any other value falls back to the
        /// unfiltered grid.
        filter: String,
        /// Absolute folder path the user is onboarding — surfaced so the
        /// catalog UI can pre-fill any "install into this site" affordance
        /// without re-resolving the active project.
        folder_path: String,
    },
    /// Ask the app shell to restore the editor in the action panel after a
    /// login flow cancel/failure. Produced by the plugin (via the
    /// `return_to_editor` Tauri command) when `promptLogin()` returns false;
    /// consumed by `app.ts`'s `onMossEvent("ReturnToEditor", …)` which clears
    /// the onboarding latch and re-mounts the editor (empty-folder onboarding
    /// cards). Routed via `emit_to("main", …)` — NOT broadcast.
    ReturnToEditor,
    /// The login browser-panel (matters.town) failed to load meaningful content:
    /// either the navigation timed out or the page body was empty after load.
    /// Carries the probe-based classification so the shell can show a calm,
    /// reason-specific failure overlay with a single **Retry** button.
    ///
    /// On receipt the shell HIDES the browser-panel webview (exposing the shell
    /// region) and renders the failure overlay.  Retry re-shows the webview and
    /// triggers a reload.  Routed via `emit_to("main", …)` — NOT broadcast.
    ///
    /// Design: C3 in
    /// docs/archive/2026-06-23-matters-login-lifecycle-and-minimal-browser-design.md
    LoginBrowserFailed { reason: LoginFailureReason },

    /// Matters is installed in the current project but has no valid session
    /// (no `boundUserName` in config.json) and the user has not intentionally
    /// dismissed the auto-open for this folder.
    ///
    /// Emitted by `spawn_process_hooks` on the first background build/preview
    /// run for a folder (before `execute_process` runs). The app shell
    /// (`app.ts`) responds by calling `connect_account("matters")` ONCE
    /// per folder session — a per-folder latch in TS prevents re-opening on
    /// every incremental rebuild.
    ///
    /// `project_path` lets the frontend tie the signal to the correct folder
    /// and verify it hasn't been superseded by a folder switch.
    ///
    /// Design: Phase 4b B3 in
    /// docs/archive/2026-06-23-matters-login-lifecycle-and-minimal-browser-design.md
    MattersNeedsConnection { project_path: String },

    /// The app UI language changed at runtime (live switch, no restart).
    ///
    /// `locale` is the FRONTEND locale tag ("en" | "zh-hans" | "zh-hant") from
    /// `AppLanguage::as_locale_str()` — hyphenated, matching the frontend
    /// `AppLocale`. This is deliberately NOT the specta-generated `AppLanguage`
    /// TS type (which is snake_case: "zh_hans"). Every app-chrome webview listens
    /// (`onMossEvent('LocaleChanged')`) and re-localizes; preview/plugin webviews
    /// have no listener and ignore it.
    LocaleChanged { locale: String },

    /// A publish-verification transition: `checking` when a publish arms (or
    /// resumes on folder open), then exactly one of `live` (the single
    /// success moment — the publish flow's green celebration keys off THIS,
    /// never off upload completion) or `unreachable` (a resting diagnosis
    /// that keeps watching and is reversed by a later `live`). Emitted on
    /// TRANSITIONS ONLY — the supervisor's fold returns at most one per
    /// probe; steady states are silent, and `get_publish_verdict` is the
    /// pull-side catch-up for a webview that mounted mid-check.
    ///
    /// Design: docs/archive/2026-08-16-publish-verified-live-design.md §2.
    PublishVerdict(PublishVerdict),

    /// The Rust-assembled record of a finished publish, replacing the
    /// `deploy_success_toast`/`announce_deploy_success` pair. Delivered
    /// through the workspace feedback queue
    /// (`queue_workspace_feedback_and_ping`, pull-only) to the preview
    /// surface — the same routing `ShowToast` uses today.
    ///
    /// Design: docs/archive/2026-09-10-publish-receipt-design.md.
    PublishReceipt(PublishReceipt),

    /// The nested-site guard is scanning the folder the user just opened —
    /// the threshold screen's checking narration. Emitted once before a scan
    /// starts (the bounded walk is synchronous and reports no live count
    /// yet), and only when a preview webview exists to render it. `token`
    /// names the guard pass; the frontend ignores events older than the
    /// newest pass it has seen. Routed via `emit_to("preview", …)` — NOT
    /// broadcast.
    ThresholdChecking { token: u32, dirs_visited: u32 },
    /// The guard's one question (nested-site threshold screen, cases
    /// (a)/(b)/(d)). The transport acks receipt
    /// (`acknowledge_threshold_prompt`) and the guard then awaits exactly
    /// one `submit_threshold_decision` quoting the prompt's token; a closed
    /// window resolves the wait to Cancel, and an un-acked prompt falls back
    /// to the dialog. Wire contract: the threshold types above, mirrored by
    /// `frontend/app/preview/threshold/seam.ts`.
    /// Routed via `emit_to("preview", …)` — NOT broadcast.
    ThresholdPrompt(ThresholdPrompt),
    /// The guard pass named by `token` ended. `cancelled: true` → the
    /// frontend restores what the overlay showed before the question;
    /// `false` → the screen stays up for the build's own overlay to paint
    /// over (no interstitial flash). A resolve for a pass the frontend is
    /// not showing is ignored — a predecessor's late teardown must not wipe
    /// its successor's live question. Routed via `emit_to("preview", …)` —
    /// NOT broadcast.
    ThresholdResolved { token: u32, cancelled: bool },

    /// A plugin rejected a stored credential and moss is asking for the
    /// replacement. The plugin's hook is parked on the answer, so the shell
    /// must resolve every one of these exactly once — with a value, or with a
    /// cancel. Routed via `emit_to("preview", …)`, like the threshold prompt
    /// and for the same reason: only the shell may answer it
    /// (`submit_credential_prompt`).
    CredentialRequest(CredentialRequest),

    /// The open folder's health verdicts, surfaced (moss#1075, phase 3 of the
    /// watcher-reliability design). Re-judged by the folder's sweep every
    /// tick (~2s) and emitted on TRANSITIONS ONLY — a healthy folder is
    /// silent, and both flags going quiet is itself a transition, so the
    /// surfaces always hear the all-clear.
    ///
    /// - `unavailable` — the sweep's "project unavailable" verdict
    ///   (`FolderSession::is_unavailable`): consecutive root-unreadable or
    ///   deadline-blown passes. The preview shows a full-window state; no
    ///   rebuilds run until the folder comes back.
    /// - `degraded` — the rebuild worker's overdue-build watchdog
    ///   (`WorkerHandle::is_degraded`): a build past its 300s deadline is
    ///   still running and the preview may lag the latest edits. Diagnostics
    ///   level: a corner-panel advisory, never a window grab.
    ///
    /// Routed via `emit_to("preview", …)` — NOT broadcast.
    FolderHealthChanged { folder: String, unavailable: bool, degraded: bool },

    /// Windows only: the invisible `HTMAXBUTTON` Snap Layouts overlay
    /// (`platform/windows/snap_overlay.rs`, Tasks 6-7 of
    /// docs/archive/2026-09-14-windows-custom-caption-design.md) was entered
    /// or left. The overlay covers the HTML maximize button and answers
    /// non-client hit-testing itself, so ordinary CSS `:hover` never fires on
    /// that button; this is the substitute signal. `hovering: true` on
    /// enter, `false` on leave — emitted only on a CHANGE, never repeated
    /// while the pointer sits still inside the overlay.
    SnapOverlayHover { hovering: bool },
    /// Companion to `SnapOverlayHover`: the overlay was clicked
    /// (`WM_NCLBUTTONUP`). No payload — the actual maximize/restore toggle is
    /// the frontend's job (`Window::toggleMaximize()`), not this event's;
    /// the overlay is a child window, and `DefWindowProcW`'s own
    /// non-client button handling has no effect on a child (only a real
    /// top-level caption maximizes itself that way).
    SnapOverlayClick,
}

/// The `PipelineEvent` -> `MossEvent` translation, with nowhere to send it yet.
///
/// Both carriers need the same translation: the desktop's `emit_tier2`
/// (app-side) and the SSE carrier's `CarrierReporter`
/// (`crate::ops::serve::events`), which runs where no `AppHandle` exists —
/// headless `moss build --serve` is what it drains.
///
/// `None` for the two events that are not moss→frontend news: `Error` is a log
/// line (written by the caller so that a second carrier draining this cannot
/// double-log it), and `Progress` is Tier-1 and rides the loading screen's own
/// channel.
pub fn moss_event_for(
    event: &crate::build::progress::PipelineEvent,
) -> Option<MossEvent> {
    use crate::build::progress::PipelineEvent;
    Some(match event {
        PipelineEvent::BackgroundProgress {
            task,
            current,
            total,
            message,
            completed,
            advisories,
        } => {
            let bg = crate::build::progress::BackgroundProgress {
                task: task.clone(),
                current: *current,
                total: *total,
                message: message.clone(),
                completed: *completed,
                advisories: advisories.clone(),
            };
            MossEvent::BackgroundProgress(bg)
        }
        PipelineEvent::AssetReady { path, asset_type } => {
            let ar = crate::build::progress::AssetReady {
                path: path.clone(),
                asset_type: asset_type.clone(),
            };
            MossEvent::AssetReady(ar)
        }
        PipelineEvent::AssetsSettled { changed } => {
            let ev = crate::build::progress::AssetsSettled { changed: changed.clone() };
            MossEvent::AssetsSettled(ev)
        }
        PipelineEvent::VideoProgress {
            filename,
            current_video,
            total_videos,
            phase,
            video_fraction,
            overall_fraction,
        } => {
            let vp = crate::build::progress::VideoConversionProgress {
                filename: filename.clone(),
                current_video: *current_video,
                total_videos: *total_videos,
                phase: phase.clone(),
                video_fraction: *video_fraction,
                overall_fraction: *overall_fraction,
            };
            MossEvent::VideoConversionProgress(vp)
        }
        PipelineEvent::BuildComplete {
            videos_converted,
            total_time_ms,
            skipped_symlinks,
        } => {
            let bc = crate::build::progress::BuildComplete {
                videos_converted: *videos_converted,
                total_time_ms: *total_time_ms,
                skipped_symlinks: *skipped_symlinks,
            };
            MossEvent::BuildComplete(bc)
        }
        PipelineEvent::FileChanged(fc) => MossEvent::FileChanged(fc.clone()),
        PipelineEvent::PreviewServerFailed { message } => MossEvent::PreviewServerFailed {
            message: message.clone(),
        },
        PipelineEvent::PluginProgress(progress) => MossEvent::PluginProgress(progress.clone()),
        // The shell's handler (`app.ts`) is keyed by the one plugin that can
        // carry `Login` today; the event name is older than the general arm.
        PipelineEvent::PluginNeedsConnection { project_path, .. } => {
            MossEvent::MattersNeedsConnection { project_path: project_path.clone() }
        }
        // Neither is moss→frontend news. `Error` is a log line, written by the
        // caller so that a second carrier draining this cannot double-log it;
        // `Progress` is Tier-1 and rides the loading screen's own channel,
        // which `TauriReporter::report` handles before calling this.
        PipelineEvent::Error { .. } | PipelineEvent::Progress { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_update_creation() {
        let progress = ProgressUpdate {
            step: "scanning".to_string(),
            message: "Scanning files...".to_string(),
            percentage: 25,
            completed: false,
            port: None,
            is_empty: None,
        };

        assert_eq!(progress.step, "scanning");
        assert_eq!(progress.message, "Scanning files...");
        assert_eq!(progress.percentage, 25);
        assert!(!progress.completed);
        assert_eq!(progress.port, None);
        assert_eq!(progress.is_empty, None);
    }

    #[test]
    fn test_progress_update_with_port() {
        let progress = ProgressUpdate {
            step: "serving".to_string(),
            message: "Server started".to_string(),
            percentage: 100,
            completed: true,
            port: Some(3000),
            is_empty: Some(false),
        };

        assert_eq!(progress.step, "serving");
        assert!(progress.completed);
        assert_eq!(progress.port, Some(3000));
        assert_eq!(progress.is_empty, Some(false));
    }

    #[test]
    fn test_progress_update_boundary_values() {
        // Test minimum percentage
        let progress_min = ProgressUpdate {
            step: "start".to_string(),
            message: "Starting...".to_string(),
            percentage: 0,
            completed: false,
            port: None,
            is_empty: None,
        };
        assert_eq!(progress_min.percentage, 0);

        // Test maximum percentage
        let progress_max = ProgressUpdate {
            step: "complete".to_string(),
            message: "Complete!".to_string(),
            percentage: 100,
            completed: true,
            port: Some(8080),
            is_empty: Some(false),
        };
        assert_eq!(progress_max.percentage, 100);
        assert!(progress_max.completed);
    }
}
