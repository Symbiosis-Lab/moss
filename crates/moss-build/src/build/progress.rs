//! Progress communication for build process
//!
//! Two-tier progress system (ADR-004):
//! - Tier 1: ProgressUpdate via Channel → loading screen (blocking phase)
//! - Tier 2: PipelineEvent variants relayed by the reporter to the frontend
//!   via either the typed MossEvent bus (post-#523) or legacy string-literal
//!   channels (still pending migration) → toast (background phase)

use serde::{Deserialize, Serialize};
use specta::Type;

/// Format progress message with optional counts
/// ADR-004: Shared utility for consistent messaging
pub fn format_progress_message(task: &str, current: u32, total: u32) -> String {
    if total > 0 {
        format!("{} ({}/{})", task, current, total)
    } else {
        task.to_string()
    }
}

/// Background progress update for non-blocking tasks
/// ADR-004: Separate from blocking ProgressUpdate for clean separation
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BackgroundProgress {
    /// Task identifier from the emitting subsystem (e.g. "markdown",
    /// "html_pages", "assets", "videos", "images", "notebooks", "rss"). The
    /// frontend panel keys task rows by `bg:${task}` and renders `message`
    /// as the label, so this set is open-ended — emitters add new ones as
    /// needed.
    pub task: String,
    /// Current item number
    pub current: u32,
    /// Total items to process
    pub total: u32,
    /// Formatted message (uses format_progress_message)
    pub message: String,
    /// Whether this task is complete
    pub completed: bool,
    /// Soft-failure advisories for the *current* iteration of this task.
    ///
    /// Producer contract: one human-readable string per item that the
    /// pipeline encountered but did not fully process this build (e.g.
    /// a notebook whose synchronous I/O timed out and was skipped). The
    /// advisory string should name the offending item and the cause —
    /// the frontend renders it verbatim in a non-progress task row.
    ///
    /// Asymmetry with `completed`: the LAST iteration of a task batch
    /// may carry `completed: true` AND a populated `advisories` vec when
    /// the final item is the one that soft-failed. Consumers MUST
    /// inspect both fields independently — `completed` answers "is this
    /// task batch done?" and `advisories` answers "what should the user
    /// know about items that didn't fully succeed?". Neither field
    /// alone disambiguates clean-success from end-with-advisories.
    ///
    /// Empty `vec![]` is the steady-state value; populated only when a
    /// soft failure occurred in the iteration this event reports.
    pub advisories: Vec<Advisory>,
}

// The advisory value objects live in `crate::advisory` (Step 3 Phase 2
// moved them out of this file so non-build producers reach them without
// depending on the build pipeline; M6a B3 moved them into the open crate,
// where the pipeline can still reach them after the crate split). The legacy
// `build::progress::*` re-export was removed in Step 3 Phase 6 once every call
// site migrated; the build pipeline now imports the names it uses privately.
use crate::advisory::{Action, Advisory, Scope, Severity};

/// Asset ready notification for placeholder swapping
/// ADR-003: Tauri events for instant updates
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AssetReady {
    /// Relative path to the asset
    pub path: String,
    /// Asset type: "image" | "video"
    pub asset_type: String,
}

/// One settled asset: its output path plus the `asset_type` the iframe-bridge
/// swap branches on ("image" | "video" | "thumbnail"). The type is
/// authoritative — derived in `crate::build::diff_settled_assets` from the
/// manifest bucket the path lives in, NOT guessed from the extension on the
/// frontend. Mirrors the `AssetReady` wire contract (`path` + `asset_type`) so
/// the frontend relays a `SettledAsset` straight into the same swap handler.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct SettledAsset {
    pub path: String,
    pub asset_type: String,
}

/// One authoritative "background asset variants have materialized" sweep,
/// emitted ONCE after a build seals (see `advertise_sealed`). Carries the set
/// of image/video output paths that are new-or-changed versus the previous
/// on-disk manifest. The frontend relays each to the iframe's page-agnostic
/// srcset cache-bust (the same swap `AssetReady` drives), turning an LQIP
/// placeholder into the real variant WITHOUT a full page reload.
///
/// Why this exists (2026-07-02): the watch refresh-diff runs on a pre-
/// background snapshot that can never contain the freshly-encoded `.webp`, so
/// dragging an image into the editor left the preview showing the placeholder
/// until a manual reload. This event fires from the one place that holds the
/// materialized manifest, after all background workers drain. Empty `changed`
/// → not emitted (preserves the no-op-when-nothing-changed contract).
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct AssetsSettled {
    /// Image/video variants that are new-or-changed, each tagged with its
    /// authoritative `asset_type` (see `SettledAsset`).
    pub changed: Vec<SettledAsset>,
}

/// Fine-grained video conversion progress for real-time UI updates.
///
/// Emitted during background video conversion to show per-video progress
/// with phase information (analyzing vs encoding) and fraction complete.
///
/// The overall_fraction formula for video `i` (0-based) of N total:
///   overall = (i + video_fraction) / N
/// Pass 1 stays at fraction 0.0 (it's fast). Pass 2 streams 0.0 -> 1.0.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct VideoConversionProgress {
    /// Filename of the video being converted (e.g., "clip.mov")
    pub filename: String,
    /// Current video number (1-based)
    pub current_video: u32,
    /// Total number of videos to convert
    pub total_videos: u32,
    /// Conversion phase: "analyzing" | "encoding"
    pub phase: String,
    /// Progress fraction within current video (0.0..1.0)
    pub video_fraction: f64,
    /// Progress fraction across all videos (0.0..1.0)
    pub overall_fraction: f64,
}

/// Build completion event - emitted when ALL tasks complete
///
/// # Architecture: Maximum Decoupling
/// Backend owns completion detection by tracking DetachedRegistry state.
/// Frontend simply listens and displays - no phase awareness needed.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct BuildComplete {
    pub videos_converted: u32,
    pub total_time_ms: u64,
    /// Number of symlinks (and macOS Finder Aliases) that were silently
    /// skipped during the asset copy phase — e.g. broken targets, escape
    /// attempts, or absolute-path targets that would 404 on deploy.
    /// Zero when all symlinks were either preserved or not present.
    pub skipped_symlinks: u32,
}

/// The generation at `current` was just swapped — the bytes a preview loads
/// from now on are this build's.
///
/// This is the ONLY event that means "the change is on the page". Note the
/// ordering it exists to expose: `BuildComplete` is emitted from
/// `await_completion`, which returns the sealed manifest BEFORE the detached
/// seal task promotes it. So a UI that sealed its progress on `BuildComplete`
/// was telling the user their change was live while `current` still pointed at
/// the previous generation.
///
/// Not a `PipelineEvent`: it is emitted from `advertise_sealed`, past the end
/// of the pipeline, and it reports a filesystem fact rather than progress.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct SitePromoted {
    /// The generation now serving at `.moss/build/current`.
    pub generation_id: String,
}

/// Unified pipeline event enum (ADR-010).
///
/// All pipeline progress goes through this type. Routed via `emit_event()`
/// (for code with a `ProgressSink`) or `BuildReporter::report` (for code with an
/// `Option<&AppHandle>`). Tier-1 `Progress` flows over an IPC `Channel`;
/// Tier-2 variants relay either to the typed `MossEvent` bus or to legacy
/// string-literal channels — see the routing table on `crate::events::MossEvent`.
///
/// Design note: `#[serde(tag = "kind")]` produces `{"kind":"Progress",...}`
/// which the frontend can switch on efficiently.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(tag = "kind")]
pub enum PipelineEvent {
    /// Tier-1 blocking phase progress (loading screen)
    Progress {
        step: String,
        message: String,
        percentage: u8,
        completed: bool,
        port: Option<u16>,
        is_empty: Option<bool>,
    },
    /// Tier-2 background task progress (toast)
    BackgroundProgress {
        task: String,
        current: u32,
        total: u32,
        message: String,
        completed: bool,
        advisories: Vec<Advisory>,
    },
    /// Individual asset ready for placeholder swap
    AssetReady {
        path: String,
        asset_type: String,
    },
    /// Post-seal sweep: image/video variants that materialized in this build
    /// (new-or-changed vs the previous manifest). Page-agnostic signal — the
    /// frontend relays each to the iframe srcset swap. See `AssetsSettled`.
    AssetsSettled {
        changed: Vec<SettledAsset>,
    },
    /// Fine-grained video conversion progress
    VideoProgress {
        filename: String,
        current_video: u32,
        total_videos: u32,
        phase: String,
        video_fraction: f64,
        overall_fraction: f64,
    },
    /// Build fully complete (all tasks including background)
    BuildComplete {
        videos_converted: u32,
        total_time_ms: u64,
        skipped_symlinks: u32,
    },
    /// File changed during watch mode (triggers smart refresh)
    FileChanged(crate::types::events::FileChangeEvent),
    /// Structured error from any pipeline phase (Gemini review gap #1)
    Error {
        phase: String,
        message: String,
    },
    /// The preview server failed to start. The shell flips the panel to an
    /// error state (`preview-state.ts`); other hosts log/relay it.
    PreviewServerFailed {
        message: String,
    },
    /// A plugin hook's progress; `completed: true` is the hook's end. The
    /// plugin manager reports it like any other pipeline event — the shell
    /// paints it on the plugin's task, the CLI prints it.
    PluginProgress(crate::plugins::types::PluginProgressEvent),
    /// A login-capable plugin has no account bound for this folder and the
    /// user has not dismissed the prompt. The shell auto-opens login ONCE; a
    /// headless build says the plugin's content will be skipped.
    PluginNeedsConnection {
        plugin: String,
        project_path: String,
    },
}

/// The ACL's routing decision (§5): a raw pipeline fact is either a Sync
/// transition (the preview's heartbeat — no Job) or a media child Job (real
/// outcome + advisories). This is the seam that stops every keystroke from
/// minting a phantom Job (invariant #6 / R4).
#[derive(Debug, Clone, PartialEq)]
pub enum RouteOutcome {
    /// Content recompiled, preview reloadable — drive Sync (hairline + flash).
    Sync,
    /// A media task with an outcome — a child Job under the Build parent.
    MediaJob {
        task: String,
        advisories: Vec<Advisory>,
    },
    /// Non-routing (in-progress tick, signal) — no transition.
    Ignore,
}

/// Media tasks that carry real outcomes/advisories and become child Jobs.
/// Everything else (markdown, html_pages, rss, notebooks, scan) is Sync detail.
const MEDIA_TASKS: &[&str] = &["images", "videos"];

/// Route a raw pipeline fact (§5 ACL) into a Sync transition, a media child
/// Job, or nothing. The split is **by task identity**: only `images`/`videos`
/// (which carry real outcomes + advisories) become child Jobs; every other
/// completed task is the preview's heartbeat (Sync). In-progress ticks and
/// signals (`AssetReady`/`FileChanged`, D7) route to `Ignore`.
pub fn route(event: &PipelineEvent) -> RouteOutcome {
    match event {
        PipelineEvent::BackgroundProgress {
            task,
            completed: true,
            advisories,
            ..
        } => {
            if MEDIA_TASKS.contains(&task.as_str()) {
                RouteOutcome::MediaJob {
                    task: task.clone(),
                    advisories: advisories.clone(),
                }
            } else {
                RouteOutcome::Sync
            }
        }
        // In-progress ticks, AssetReady, FileChanged, VideoProgress detail: no
        // transition here (AssetReady/FileChanged stay signals — D7).
        _ => RouteOutcome::Ignore,
    }
}

/// Map a completed media `BackgroundProgress`'s advisories to a child Job's
/// terminal state. Advisories flow into `Done.advisories` (invariant #2); a
/// Blocking one flips to `Failed` via the smart constructor (#1) — the ACL does
/// not re-decide this, it delegates to `TaskState::done`.
pub fn media_child_job_state(advisories: Vec<Advisory>) -> crate::tasks::TaskState {
    crate::tasks::TaskState::done(None, advisories)
}

/// Spawn and terminalize a media child Job under the parent Build Job, at a
/// media task's completion emit.
///
/// **Real-work gate (FIX 1b).** No-op on zero conversions AND zero advisories.
/// `collect_videos_for_conversion` is not change-filtered, so a content-only
/// rebuild on a video site runs the worker, cache-skips every video and arrives
/// here having done nothing; minting a child — and through `ensure_build_parent`
/// a parent — would be a phantom Job (invariant #6).
///
/// The parent is minted lazily, immediately before the child, which preserves
/// parent-before-child ordering. Idempotent across both media workers —
/// whichever does real work first spawns it. No-op headlessly (no registry).
pub(crate) fn spawn_media_child_job(
    services: &crate::types::services::BuildServices,
    task: &str,
    converted_count: u32,
    advisories: Vec<Advisory>,
) {
    // FIX 1b: gate on ACTUAL work. A cache-skipped content rebuild (zero real
    // conversions, zero advisories) mints no Job — and never spawns the parent.
    if converted_count == 0 && advisories.is_empty() {
        return;
    }
    if !MEDIA_TASKS.contains(&task) {
        return;
    }
    let Some(registry) = services.task_registry.as_ref() else {
        return;
    };
    let Some(parent) = services.ensure_build_parent() else {
        return;
    };
    let child = registry.spawn_child(
        crate::tasks::WindowId::from("main"),
        crate::tasks::TaskScope::Preview,
        crate::tasks::TaskKind::AssetTransform,
        crate::tasks::TaskTone::Ambient,
        parent,
    );
    let verb = crate::tasks::Verb::core(crate::tasks::Verb::BUILT);
    child.set_verb(verb);
    child.set_state(media_child_job_state(advisories));
}

/// Route a pipeline event through the configured sink.
///
/// Kept as a free function because eleven call sites read `emit_event(sink, &ev)`
/// and the indirection costs nothing; the routing itself now lives in each
/// `BuildReporter` implementation rather than in a match over an enum that
/// named tauri.
pub fn emit_event(sink: &super::ProgressSink, event: &PipelineEvent) {
    sink.report(event)
}

/// Say that this build is blocked on plugin process hooks, and later that it
/// is not.
///
/// Through the **progress sink**, not `log`. A CLI build installs no `log`
/// implementation at all: the only logger moss ever registers is
/// `tauri_plugin_log`, built inside the `tauri::Builder` chain that
/// `startup::headless` skips — so every `log::info!` on this path is dropped on
/// the floor. That is what happened to the pair of `[diag] preview-wait:` lines
/// these replace. They were the CLI's only sign that `moss build
/// --wait-plugins` had entered the blocking wait, they rode on the logger the
/// Tauri path happened to install, and #1019 took the Tauri path away from a
/// plugin-bearing build without anyone noticing the announcement went with it.
///
/// The sink is the right channel regardless: it reaches the CLI's stderr AND
/// the GUI loading screen, and it is the mode-agnostic seam ADR-010 asks the
/// pipeline to branch through. Nothing is lost from the app's log either —
/// `PhaseTrace::start("process_hooks_await")` still records the same boundary
/// and elapsed time wherever a logger exists.
///
/// This is the one phase of a build that legitimately runs for minutes (a
/// Matters sync; the phase's own budget is 60 s), so silence here reads as a
/// hang. `step: "plugin"` reuses the vocabulary the generate hook already emits
/// rather than minting a surface the frontend has to learn.
pub fn announce_process_hook_wait(sink: &super::ProgressSink, port: Option<u16>) {
    emit_event(
        sink,
        &PipelineEvent::Progress {
            step: "plugin".to_string(),
            message: "Waiting for plugin process hooks to complete...".to_string(),
            percentage: 6,
            completed: false,
            port,
            is_empty: None,
        },
    );
}

/// The other half of [`announce_process_hook_wait`] — the await returned.
///
/// `completed: false` even though the wait is over: on this event `completed`
/// means "the BUILD is done" (`StdoutReporter` prints it as `[done] ...`, the
/// loading screen dismisses on it), and the build has barely started.
pub fn announce_process_hook_done(sink: &super::ProgressSink, elapsed_ms: u128, port: Option<u16>) {
    emit_event(
        sink,
        &PipelineEvent::Progress {
            step: "plugin".to_string(),
            message: format!("Plugin process hooks completed in {} ms", elapsed_ms),
            percentage: 8,
            completed: false,
            port,
            is_empty: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::ports::reporter::BuildReporter;

    /// Keeps every event a sink was handed, so a test can assert on what the
    /// pipeline *said* rather than on where it said it.
    #[derive(Default)]
    struct RecordingReporter(std::sync::Mutex<Vec<PipelineEvent>>);

    impl BuildReporter for RecordingReporter {
        fn report(&self, event: &PipelineEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
        fn is_terminal(&self) -> bool {
            true
        }
    }

    fn messages(rec: &std::sync::Arc<RecordingReporter>) -> Vec<String> {
        rec.0
            .lock()
            .unwrap()
            .iter()
            .map(|e| match e {
                PipelineEvent::Progress { message, .. } => message.clone(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    /// INVARIANT: a build that blocks on plugin process hooks says so through
    /// the progress SINK, which needs no logger.
    ///
    /// Guards the regression that made `moss build --wait-plugins` silent: the
    /// announcement used to be a `log::info!`, and a headless CLI build
    /// registers no `log` implementation at all (`tauri_plugin_log` lives
    /// inside the `tauri::Builder` chain `startup::headless` skips). Asserting
    /// on the sink is what makes the announcement survive having no logger.
    #[test]
    fn process_hook_wait_is_announced_through_the_sink() {
        let rec = std::sync::Arc::new(RecordingReporter::default());
        let sink: crate::build::ProgressSink = rec.clone();

        announce_process_hook_wait(&sink, None);
        announce_process_hook_done(&sink, 1234, None);

        let msgs = messages(&rec);
        assert_eq!(msgs.len(), 2, "both halves of the wait must be announced");
        assert!(
            msgs[0].contains("Waiting for plugin process hooks"),
            "entering the wait must be announced, got {:?}",
            msgs[0]
        );
        assert!(
            msgs[1].contains("Plugin process hooks completed") && msgs[1].contains("1234"),
            "leaving the wait must be announced with its elapsed time, got {:?}",
            msgs[1]
        );
    }

    /// INVARIANT: neither announcement claims the BUILD finished.
    ///
    /// `completed: true` is the terminal signal — `StdoutReporter` prints it as
    /// `[done] ...` and the loading screen dismisses on it. These fire before
    /// rendering has started, so setting it would end the build on screen while
    /// the build kept running.
    #[test]
    fn process_hook_announcements_are_not_terminal() {
        let rec = std::sync::Arc::new(RecordingReporter::default());
        let sink: crate::build::ProgressSink = rec.clone();

        announce_process_hook_wait(&sink, Some(1420));
        announce_process_hook_done(&sink, 0, Some(1420));

        for event in rec.0.lock().unwrap().iter() {
            let PipelineEvent::Progress { step, percentage, completed, port, .. } = event else {
                panic!("expected a Progress event, got {event:?}");
            };
            assert!(!completed, "a mid-build phase must not signal completion");
            assert_eq!(step, "plugin", "reuse the step the plugin phase already emits");
            assert!(*percentage < 10, "the wait precedes the 10% build event");
            assert_eq!(*port, Some(1420), "the live port must ride along");
        }
    }

    #[test]
    fn test_format_progress_message_with_counts() {
        let msg = format_progress_message("Copying assets", 45, 134);
        assert_eq!(msg, "Copying assets (45/134)");
    }

    #[test]
    fn test_format_progress_message_without_counts() {
        let msg = format_progress_message("Finalizing", 0, 0);
        assert_eq!(msg, "Finalizing");
    }

    #[test]
    fn test_format_progress_message_at_start() {
        let msg = format_progress_message("Converting videos", 1, 10);
        assert_eq!(msg, "Converting videos (1/10)");
    }

    #[test]
    fn test_format_progress_message_at_end() {
        let msg = format_progress_message("Converting videos", 10, 10);
        assert_eq!(msg, "Converting videos (10/10)");
    }

    #[test]
    fn test_background_progress_serializes_correctly() {
        let progress = BackgroundProgress {
            task: "assets".to_string(),
            current: 10,
            total: 100,
            message: "Copying assets (10/100)".to_string(),
            completed: false,
            advisories: vec![],
        };
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"task\":\"assets\""));
        assert!(json.contains("\"current\":10"));
        assert!(json.contains("\"total\":100"));
        assert!(json.contains("\"completed\":false"));
    }

    #[test]
    fn test_background_progress_with_advisories() {
        let progress = BackgroundProgress {
            task: "videos".to_string(),
            current: 0,
            total: 3,
            message: "Converting videos (0/3)".to_string(),
            completed: false,
            advisories: vec![Advisory {
                scope: Scope::Environment,
                severity: Severity::NeedsAction,
                item: None,
                what: "ffmpeg not found".into(),
                action: Action::None,
            }],
        };
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"what\":\"ffmpeg not found\""));
    }

    #[test]
    fn advisory_serializes_per_file_and_build_wide() {
        let per_file = Advisory {
            scope: Scope::File,
            severity: Severity::ShippedDegraded,
            item: Some("clip.MOV".into()),
            what: "unsupported codec".into(),
            action: Action::None,
        };
        let json = serde_json::to_string(&per_file).unwrap();
        assert!(json.contains("\"item\":\"clip.MOV\""));
        assert!(json.contains("\"what\":\"unsupported codec\""));

        let build_wide = Advisory {
            scope: Scope::Environment,
            severity: Severity::NeedsAction,
            item: None,
            what: "FFmpeg not available".into(),
            action: Action::None,
        };
        let json = serde_json::to_string(&build_wide).unwrap();
        assert!(json.contains("\"item\":null"));

        let back: Advisory = serde_json::from_str(&json).unwrap();
        assert_eq!(back.item, None);
        assert_eq!(back.what, "FFmpeg not available");
    }

    #[test]
    fn advisory_round_trips_with_command_action() {
        let adv = Advisory {
            scope: Scope::Environment,
            severity: Severity::NeedsAction,
            item: None,
            what: "Videos shipped full-size — FFmpeg isn't installed".into(),
            action: Action::Command {
                run: "brew install ffmpeg".into(),
                label: "Copy".into(),
            },
        };
        let json = serde_json::to_string(&adv).unwrap();
        // Enums tag as expected (serde default external tagging).
        assert!(json.contains("\"scope\":\"Environment\""), "{json}");
        assert!(json.contains("\"severity\":\"NeedsAction\""), "{json}");
        assert!(json.contains("\"action\":{\"Command\":"), "{json}");
        assert!(json.contains("\"run\":\"brew install ffmpeg\""), "{json}");
        assert!(json.contains("\"label\":\"Copy\""), "{json}");

        let back: Advisory = serde_json::from_str(&json).unwrap();
        assert_eq!(back.what, adv.what);
        match back.action {
            Action::Command { run, label } => {
                assert_eq!(run, "brew install ffmpeg");
                assert_eq!(label, "Copy");
            }
            other => panic!("expected Command action, got {other:?}"),
        }
    }

    #[test]
    fn test_asset_ready_serializes_correctly() {
        let ready = AssetReady {
            path: "images/photo.jpg".to_string(),
            asset_type: "image".to_string(),
        };
        let json = serde_json::to_string(&ready).unwrap();
        assert!(json.contains("\"path\":\"images/photo.jpg\""));
        assert!(json.contains("\"asset_type\":\"image\""));
    }

    #[test]
    fn test_asset_ready_video_type() {
        let ready = AssetReady {
            path: "videos/clip.mp4".to_string(),
            asset_type: "video".to_string(),
        };
        assert_eq!(ready.asset_type, "video");
    }

    #[test]
    fn test_build_complete_serializes_correctly() {
        let complete = BuildComplete {
            videos_converted: 5,
            total_time_ms: 12345,
            skipped_symlinks: 0,
        };
        let json = serde_json::to_string(&complete).unwrap();
        assert!(json.contains("\"videos_converted\":5"));
        assert!(json.contains("\"total_time_ms\":12345"));
    }

    #[test]
    fn test_build_complete_with_no_videos() {
        let complete = BuildComplete {
            videos_converted: 0,
            total_time_ms: 1500,
            skipped_symlinks: 0,
        };
        assert_eq!(complete.videos_converted, 0);
        assert_eq!(complete.total_time_ms, 1500);
    }

    #[test]
    fn test_build_complete_with_multiple_videos() {
        let complete = BuildComplete {
            videos_converted: 15,
            total_time_ms: 45000,
            skipped_symlinks: 0,
        };
        assert_eq!(complete.videos_converted, 15);
        assert_eq!(complete.total_time_ms, 45000);
    }

    #[test]
    fn test_build_complete_serializes_skipped_symlinks() {
        let complete = BuildComplete {
            videos_converted: 0,
            total_time_ms: 1000,
            skipped_symlinks: 3,
        };
        let json = serde_json::to_string(&complete).unwrap();
        assert!(json.contains("\"skipped_symlinks\":3"));
        assert_eq!(complete.skipped_symlinks, 3);
    }

    // =========================================================================
    // VideoConversionProgress Tests
    // =========================================================================

    #[test]
    fn test_video_conversion_progress_serializes_correctly() {
        let progress = VideoConversionProgress {
            filename: "clip.mov".to_string(),
            current_video: 2,
            total_videos: 5,
            phase: "encoding".to_string(),
            video_fraction: 0.75,
            overall_fraction: 0.35,
        };
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"filename\":\"clip.mov\""));
        assert!(json.contains("\"current_video\":2"));
        assert!(json.contains("\"total_videos\":5"));
        assert!(json.contains("\"phase\":\"encoding\""));
        assert!(json.contains("\"video_fraction\":0.75"));
        assert!(json.contains("\"overall_fraction\":0.35"));
    }

    #[test]
    fn test_video_conversion_progress_analyzing_phase() {
        let progress = VideoConversionProgress {
            filename: "intro.mov".to_string(),
            current_video: 1,
            total_videos: 3,
            phase: "analyzing".to_string(),
            video_fraction: 0.0,
            overall_fraction: 0.0,
        };
        assert_eq!(progress.phase, "analyzing");
        assert_eq!(progress.video_fraction, 0.0);
        assert_eq!(progress.overall_fraction, 0.0);
    }

    #[test]
    fn test_video_conversion_progress_overall_fraction_formula() {
        // For video i (0-based) of N total: overall = (i + video_fraction) / N
        let total: u32 = 4;

        // First video (index 0), 50% done
        let index: u32 = 0;
        let video_fraction = 0.5;
        let overall = (index as f64 + video_fraction) / total as f64;
        assert!((overall - 0.125).abs() < f64::EPSILON);

        // Third video (index 2), 75% done
        let index: u32 = 2;
        let video_fraction = 0.75;
        let overall = (index as f64 + video_fraction) / total as f64;
        assert!((overall - 0.6875).abs() < f64::EPSILON);

        // Last video (index 3), 100% done
        let index: u32 = 3;
        let video_fraction = 1.0;
        let overall = (index as f64 + video_fraction) / total as f64;
        assert!((overall - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_video_conversion_progress_deserializes_correctly() {
        let json = r#"{
            "filename": "test.mov",
            "current_video": 3,
            "total_videos": 10,
            "phase": "encoding",
            "video_fraction": 0.42,
            "overall_fraction": 0.242
        }"#;
        let progress: VideoConversionProgress = serde_json::from_str(json).unwrap();
        assert_eq!(progress.filename, "test.mov");
        assert_eq!(progress.current_video, 3);
        assert_eq!(progress.total_videos, 10);
        assert_eq!(progress.phase, "encoding");
        assert!((progress.video_fraction - 0.42).abs() < f64::EPSILON);
        assert!((progress.overall_fraction - 0.242).abs() < f64::EPSILON);
    }

    #[test]
    fn test_video_conversion_progress_clone() {
        let progress = VideoConversionProgress {
            filename: "demo.mov".to_string(),
            current_video: 1,
            total_videos: 1,
            phase: "encoding".to_string(),
            video_fraction: 0.5,
            overall_fraction: 0.5,
        };
        let cloned = progress.clone();
        assert_eq!(cloned.filename, progress.filename);
        assert_eq!(cloned.current_video, progress.current_video);
        assert_eq!(cloned.video_fraction, progress.video_fraction);
    }

    #[test]
    fn test_emit_tier2_video_progress_with_none() {
        // Should not panic when nobody is listening
        crate::build::ports::reporter::NullReporter.report(&PipelineEvent::VideoProgress {
            filename: "test.mov".to_string(),
            current_video: 1,
            total_videos: 1,
            phase: "encoding".to_string(),
            video_fraction: 0.0,
            overall_fraction: 0.0,
        });
    }

    // =========================================================================
    // PipelineEvent Tests (Phase 3)
    // =========================================================================

    #[test]
    fn test_pipeline_event_progress_serializes_with_kind_tag() {
        let event = PipelineEvent::Progress {
            step: "scanning".to_string(),
            message: "Scanning folder...".to_string(),
            percentage: 20,
            completed: false,
            port: None,
            is_empty: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"kind\":\"Progress\""),
            "Should have kind tag: {}",
            json
        );
        assert!(
            json.contains("\"step\":\"scanning\""),
            "Should have step field: {}",
            json
        );
    }

    #[test]
    fn test_pipeline_event_background_progress_serializes() {
        let event = PipelineEvent::BackgroundProgress {
            task: "assets".to_string(),
            current: 5,
            total: 10,
            message: "Copying assets (5/10)".to_string(),
            completed: false,
            advisories: vec![],
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"BackgroundProgress\""));
        assert!(json.contains("\"task\":\"assets\""));
    }

    #[test]
    fn test_pipeline_event_asset_ready_serializes() {
        let event = PipelineEvent::AssetReady {
            path: "images/logo.png".to_string(),
            asset_type: "image".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"AssetReady\""));
        assert!(json.contains("\"path\":\"images/logo.png\""));
    }

    #[test]
    fn test_pipeline_event_video_progress_serializes() {
        let event = PipelineEvent::VideoProgress {
            filename: "clip.mov".to_string(),
            current_video: 2,
            total_videos: 5,
            phase: "encoding".to_string(),
            video_fraction: 0.75,
            overall_fraction: 0.35,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"VideoProgress\""));
        assert!(json.contains("\"filename\":\"clip.mov\""));
    }

    #[test]
    fn test_pipeline_event_build_complete_serializes() {
        let event = PipelineEvent::BuildComplete {
            videos_converted: 3,
            total_time_ms: 5000,
            skipped_symlinks: 0,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"BuildComplete\""));
        assert!(json.contains("\"videos_converted\":3"));
    }

    #[test]
    fn test_pipeline_event_file_changed_serializes() {
        let mut fce = crate::types::events::FileChangeEvent::new();
        fce.set_changed_output_files(vec!["index.html".to_string()]);
        let event = PipelineEvent::FileChanged(fce);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"FileChanged\""));
        assert!(json.contains("index.html"));
    }

    #[test]
    fn test_pipeline_event_error_serializes() {
        let event = PipelineEvent::Error {
            phase: "build".to_string(),
            message: "Template not found".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"Error\""));
        assert!(json.contains("\"phase\":\"build\""));
    }

    #[test]
    fn test_pipeline_event_all_variants_deserialize() {
        // Verify round-trip for each variant
        let events = vec![
            PipelineEvent::Progress {
                step: "test".to_string(),
                message: "Testing".to_string(),
                percentage: 50,
                completed: false,
                port: Some(3000),
                is_empty: Some(false),
            },
            PipelineEvent::BackgroundProgress {
                task: "videos".to_string(),
                current: 1,
                total: 3,
                message: "Converting".to_string(),
                completed: false,
                advisories: vec![Advisory {
                    scope: Scope::File,
                    severity: Severity::ShippedDegraded,
                    item: Some("nb1.ipynb".into()),
                    what: "warn1".into(),
                    action: Action::None,
                }],
            },
            PipelineEvent::AssetReady {
                path: "test.png".to_string(),
                asset_type: "image".to_string(),
            },
            PipelineEvent::VideoProgress {
                filename: "test.mov".to_string(),
                current_video: 1,
                total_videos: 1,
                phase: "encoding".to_string(),
                video_fraction: 0.5,
                overall_fraction: 0.5,
            },
            PipelineEvent::BuildComplete {
                videos_converted: 0,
                total_time_ms: 100,
                skipped_symlinks: 0,
            },
            PipelineEvent::Error {
                phase: "scan".to_string(),
                message: "Not found".to_string(),
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let deserialized: PipelineEvent = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&deserialized).unwrap();
            assert_eq!(json, json2, "Round-trip should be identical for {:?}", event);
        }
    }

    #[test]
    fn test_emit_event_null_sink_discards() {
        // Null sink should not panic for any variant
        let sink = crate::build::null_sink();
        let events = vec![
            PipelineEvent::Progress {
                step: "test".to_string(),
                message: "msg".to_string(),
                percentage: 0,
                completed: false,
                port: None,
                is_empty: None,
            },
            PipelineEvent::BackgroundProgress {
                task: "t".to_string(),
                current: 0,
                total: 0,
                message: "m".to_string(),
                completed: false,
                advisories: vec![],
            },
            PipelineEvent::Error {
                phase: "p".to_string(),
                message: "e".to_string(),
            },
        ];
        for event in &events {
            emit_event(&sink, event); // Should not panic
        }
    }

    // =========================================================================
    // Design Invariant Tests (ADR-010)
    // =========================================================================

    /// INVARIANT: Stdout sink handles ALL PipelineEvent variants without panic.
    /// Guards: CLI mode can display any event type.
    #[test]
    fn test_emit_event_stdout_sink_all_variants() {
        let sink = crate::build::stdout_sink();
        let events = vec![
            PipelineEvent::Progress {
                step: "scanning".into(), message: "msg".into(),
                percentage: 20, completed: false, port: Some(3000), is_empty: Some(false),
            },
            PipelineEvent::Progress {
                step: "complete".into(), message: "done".into(),
                percentage: 100, completed: true, port: None, is_empty: None,
            },
            PipelineEvent::BackgroundProgress {
                task: "assets".into(), current: 5, total: 10,
                message: "msg".into(), completed: false, advisories: vec![],
            },
            PipelineEvent::BackgroundProgress {
                task: "assets".into(), current: 10, total: 10,
                message: "done".into(), completed: true, advisories: vec![Advisory {
                    scope: Scope::File,
                    severity: Severity::ShippedDegraded,
                    item: Some("a.mov".into()),
                    what: "w".into(),
                    action: Action::None,
                }],
            },
            PipelineEvent::AssetReady { path: "img.png".into(), asset_type: "image".into() },
            PipelineEvent::VideoProgress {
                filename: "v.mov".into(), current_video: 1, total_videos: 2,
                phase: "encoding".into(), video_fraction: 0.5, overall_fraction: 0.25,
            },
            PipelineEvent::BuildComplete { videos_converted: 3, total_time_ms: 5000, skipped_symlinks: 0 },
            PipelineEvent::FileChanged(crate::types::events::FileChangeEvent::new()),
            PipelineEvent::Error { phase: "build".into(), message: "err".into() },
        ];
        for event in &events {
            emit_event(&sink, event); // Must not panic
        }
    }

    /// INVARIANT: a discarding reporter is a safe no-op for every variant.
    /// Guards: Headless mode does not crash on progress emission.
    #[test]
    fn test_emit_tier2_noop_with_none() {
        let sink = crate::build::ports::reporter::HeadlessReporter;
        sink.report(&PipelineEvent::BackgroundProgress {
            task: "test".into(),
            current: 0,
            total: 0,
            message: "test".into(),
            completed: false,
            advisories: vec![],
        });
        sink.report(&PipelineEvent::AssetReady {
            path: "path".into(),
            asset_type: "image".into(),
        });
        sink.report(&PipelineEvent::VideoProgress {
            filename: "test.mov".into(),
            current_video: 1,
            total_videos: 1,
            phase: "encoding".into(),
            video_fraction: 0.0,
            overall_fraction: 0.0,
        });
        sink.report(&PipelineEvent::BuildComplete {
            videos_converted: 0,
            total_time_ms: 0,
            skipped_symlinks: 0,
        });
        // All survived — invariant holds
    }

    // ── Step 3 Phase 4: media child Jobs + Build parent receipt ───────────

    use crate::tasks::{TaskKind, TaskScope, TaskState, WindowId};
    use crate::types::services::BuildServices;

    fn degraded() -> Advisory {
        Advisory {
            scope: Scope::File,
            severity: Severity::ShippedDegraded,
            item: Some("clip.mov".into()),
            what: "shipped unoptimized".into(),
            action: Action::None,
        }
    }

    #[test]
    fn real_media_work_lazily_mints_the_parent_then_a_child_under_it() {
        // FIX 1a/1b: the parent is NOT pre-spawned. A real conversion (count > 0)
        // mints the parent lazily, then a child Job nested under it.
        let (svc, registry) = BuildServices::with_task_registry();
        spawn_media_child_job(&svc, "videos", 1, vec![degraded()]);
        let tasks = registry.tasks(&WindowId::from("main"), TaskScope::Preview);
        // Exactly one parent (TaskKind::Build) + one child under it.
        let parent = tasks
            .iter()
            .find(|t| t.kind == TaskKind::Build)
            .expect("real work must lazily mint the Build parent");
        let child = tasks
            .iter()
            .find(|t| t.parent == Some(parent.id))
            .expect("a media completion with real work must mint a child Job under the parent");
        // The child terminal carries the advisory (invariant #2).
        match &child.state {
            TaskState::Succeeded { advisories, .. } => assert_eq!(advisories.len(), 1),
            other => panic!("expected Succeeded child carrying the advisory, got {other:?}"),
        }
    }

    #[test]
    fn an_advisory_with_zero_conversions_still_mints_a_child() {
        // The FFmpeg-missing fallback converts nothing but raises a NeedsAction
        // advisory — that IS real work, so the gate passes on advisories alone.
        let (svc, registry) = BuildServices::with_task_registry();
        spawn_media_child_job(&svc, "videos", 0, vec![degraded()]);
        let tasks = registry.tasks(&WindowId::from("main"), TaskScope::Preview);
        assert!(
            tasks.iter().any(|t| t.kind == TaskKind::Build),
            "an advisory is real work ⇒ the parent is minted"
        );
        assert!(
            tasks.iter().any(|t| t.parent.is_some()),
            "an advisory is real work ⇒ a child Job is minted"
        );
    }

    #[test]
    fn content_only_rebuild_on_a_video_site_mints_zero_jobs() {
        // THE invariant-#6 gate (FIX 1b): a content-only rebuild on a video site
        // runs the video worker, every video CACHE-SKIPS (zero real conversions),
        // and no advisories are raised. The completion call reaches the gate with
        // converted_count == 0 && advisories.is_empty() and must mint NOTHING —
        // no lazy parent, no child Job. This drives the REAL backend gate, not the
        // route() unit or the frontend burst test.
        let (svc, registry) = BuildServices::with_task_registry();
        spawn_media_child_job(&svc, "videos", 0, vec![]);
        let tasks = registry.tasks(&WindowId::from("main"), TaskScope::Preview);
        assert!(
            tasks.is_empty(),
            "a cache-skipped content rebuild must mint zero Jobs (no parent, no child), got {tasks:?}"
        );
        assert!(
            svc.current_build_parent().is_none(),
            "the lazy parent must never be spawned on a content-only rebuild"
        );
    }

    // (`finish_build_no_longer_touches_the_job_side` retired with `finish_build`
    // itself: this module no longer emits ANY terminal receipt, which the
    // `BuildTerminalBarrier` tests in `background.rs` now assert from the owning
    // side rather than the abstaining one.)
}

/// The one shape an advisory-only notice takes: a completed background step
/// under `task`, carrying advisories and nothing else, its message the first
/// advisory's text. `None` when there is nothing to say, so every caller can
/// build its list unconditionally and hand it over.
fn advisory_event(task: &str, advisories: Vec<Advisory>) -> Option<PipelineEvent> {
    let message = advisories.first()?.what.clone();
    Some(PipelineEvent::BackgroundProgress {
        task: task.into(),
        current: 0,
        total: 0,
        message,
        completed: true,
        advisories,
    })
}

/// Build a `BackgroundProgress` advisory event for skipped symlinks.
///
/// When `count > 0` a NeedsAction advisory is returned so the L1 hairline dot
/// shows it without a toast. Returns `None` when no symlinks were skipped
/// (steady-state clean build).
pub fn make_symlink_skip_advisory(count: u32) -> Option<PipelineEvent> {
    if count == 0 {
        return None;
    }
    let key = if count == 1 {
        "symlinks_skipped_one"
    } else {
        "symlinks_skipped_many"
    };
    advisory_event(
        "assets",
        vec![Advisory {
            scope: Scope::Config,
            severity: Severity::NeedsAction,
            item: None,
            what: crate::infra::app_advisory::fmt(key, &[("count", &count.to_string())]),
            action: Action::None,
        }],
    )
}

/// Emit [`make_config_version_ahead_advisory`] for an already-parsed config,
/// if it applies. Takes the parsed `ConfigFile` directly — never a path —
/// so `build_inner`'s one read+parse of config.toml ("H-config") can feed
/// this without a second read; `config::ConfigFile::schema_version_ahead`
/// is free against a table already in hand. `run_vault_migrations` (run
/// once at the entry point, before H-config) already logged the same
/// condition once per vault per session; this is the author-facing twin,
/// re-derived every build (not deduped) so it clears the moment the config
/// is fixed or moss is updated, same as every other advisory. `cfg: None`
/// (a config that could not be read) is silently not this advisory's story
/// to tell — something downstream already owns surfacing a real read error.
pub fn report_config_version_ahead(
    reporter: Option<&dyn super::ports::reporter::BuildReporter>,
    cfg: Option<&crate::config::ConfigFile>,
) {
    let Some(found) = cfg.and_then(|c| c.schema_version_ahead()) else {
        return;
    };
    let Some(event) = make_config_version_ahead_advisory(found) else {
        return;
    };
    if let Some(reporter) = reporter {
        reporter.report(&event);
    }
}

/// Build a `BackgroundProgress` advisory event for a config.toml a newer
/// moss already stamped. `NeedsAction`, not `Blocking`: the build itself
/// still renders (with the raw, unmigrated table — see
/// `site_config::config_schema_version_ahead`'s doc), so this is a preview-
/// time notice, not a build failure. Publish is the one path that refuses
/// outright (`deploy/push.rs`), because shipping the defaulted config to a
/// live site is the one consequence preview's "still shows something" excuse
/// doesn't cover.
pub fn make_config_version_ahead_advisory(found: u32) -> Option<PipelineEvent> {
    advisory_event(
        "config",
        vec![Advisory {
            scope: Scope::Config,
            severity: Severity::NeedsAction,
            item: None,
            what: crate::infra::app_advisory::fmt(
                "config_schema_version_ahead",
                &[
                    ("found", &found.to_string()),
                    ("max", &crate::config::migrations::CURRENT_VERSION.to_string()),
                ],
            ),
            action: Action::None,
        }],
    )
}

// `make_live_record_advisory` stood here and was deleted on 2026-08-29 — see
// the tombstone in `infra::app_advisory` for why. It is a log fact now.

// `make_uid_deferred_advisory` stood here and was deleted on 2026-08-30. A
// deferral is now rare — `resolve_duplicate_uids` re-mints any contender this
// install has never built, which is the shape a duplicated note makes — and
// what is left is not the author's to diagnose or fix. A note ID is moss's to
// manage; the remaining collisions go to the log.

/// Build a `BackgroundProgress` advisory event for duplicate-uid rewrites.
///
/// Only the reassignments flagged `live_thread_at_risk` — the deployed snapshot
/// shows the uid live, but could not say which file is the published page — so
/// moss's choice of keeper may have sent existing comments to the wrong page.
/// That is the one case the author can check and moss cannot.
///
/// The other reassignments are not shown (2026-08-30). Either nothing under
/// that ID was ever published, or the record named the keeper by exact path: a
/// copy quietly getting its own identity is bookkeeping, not news. Every
/// reassignment is still logged at the call site, at-risk or not.
///
/// Returns `None` when nothing at risk was reassigned.
pub fn make_duplicate_uid_advisory(
    reassignments: &[crate::build::render::uid_dedup::UidReassignment],
) -> Option<PipelineEvent> {
    let advisories: Vec<Advisory> = reassignments
        .iter()
        .filter(|r| r.live_thread_at_risk)
        .map(|r| Advisory {
            scope: Scope::File,
            severity: Severity::NeedsAction,
            item: Some(r.reassigned_path.clone()),
            what: crate::infra::app_advisory::fmt(
                "duplicate_note_id_live",
                &[("keeper", &r.keeper_path), ("dup", &r.reassigned_path)],
            ),
            // The one advisory in moss that reports an act rather than a
            // condition, and so the one that must outlive the build that
            // raised it. See `Action::Acknowledge`.
            action: Action::Acknowledge {
                label: crate::infra::app_advisory::fmt("acknowledged_label", &[]),
            },
        })
        .collect();
    advisory_event("markdown", advisories)
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use crate::advisory::{Action, Advisory, Scope, Severity};

    #[test]
    fn content_only_rebuild_births_zero_jobs() {
        // A markdown/html recompile is the preview's heartbeat (Sync), NOT a Job.
        let outcome = route(&PipelineEvent::BackgroundProgress {
            task: "markdown".into(),
            current: 10,
            total: 10,
            message: "done".into(),
            completed: true,
            advisories: vec![],
        });
        assert!(matches!(outcome, RouteOutcome::Sync));
    }

    #[test]
    fn media_work_births_a_child_job() {
        let outcome = route(&PipelineEvent::BackgroundProgress {
            task: "videos".into(),
            current: 6,
            total: 6,
            message: "done".into(),
            completed: true,
            advisories: vec![],
        });
        assert!(matches!(outcome, RouteOutcome::MediaJob { .. }));
    }

    #[test]
    fn images_completed_births_a_child_job() {
        let outcome = route(&PipelineEvent::BackgroundProgress {
            task: "images".into(),
            current: 30,
            total: 30,
            message: "done".into(),
            completed: true,
            advisories: vec![],
        });
        assert!(matches!(outcome, RouteOutcome::MediaJob { .. }));
    }

    #[test]
    fn media_job_carries_task_and_advisories_through() {
        let advisory = Advisory {
            scope: Scope::File,
            severity: Severity::ShippedDegraded,
            item: Some("clip.mov".into()),
            what: "shipped unoptimized".into(),
            action: Action::None,
        };
        let outcome = route(&PipelineEvent::BackgroundProgress {
            task: "videos".into(),
            current: 6,
            total: 6,
            message: "done".into(),
            completed: true,
            advisories: vec![advisory.clone()],
        });
        match outcome {
            RouteOutcome::MediaJob { task, advisories } => {
                assert_eq!(task, "videos");
                assert_eq!(advisories.len(), 1);
                assert_eq!(advisories[0].what, "shipped unoptimized");
            }
            other => panic!("expected MediaJob, got {other:?}"),
        }
    }

    #[test]
    fn in_progress_media_tick_is_ignored() {
        // A non-completed media tick is detail, not a terminal child Job.
        let outcome = route(&PipelineEvent::BackgroundProgress {
            task: "videos".into(),
            current: 3,
            total: 6,
            message: "encoding".into(),
            completed: false,
            advisories: vec![],
        });
        assert!(matches!(outcome, RouteOutcome::Ignore));
    }

    #[test]
    fn asset_ready_is_ignored_stays_a_signal() {
        // D7: AssetReady is a signal, never a Job — the `_ => Ignore` arm covers it.
        let outcome = route(&PipelineEvent::AssetReady {
            path: "img/a.png".into(),
            asset_type: "image".into(),
        });
        assert!(matches!(outcome, RouteOutcome::Ignore));
    }

    #[test]
    fn assets_settled_is_ignored_stays_a_signal() {
        // AssetsSettled is a page-agnostic preview signal, never a Job — the
        // route() `_ => Ignore` arm must cover it so it can't mint a phantom Job.
        let outcome = route(&PipelineEvent::AssetsSettled {
            changed: vec![SettledAsset { path: "assets/pic.webp".into(), asset_type: "image".into() }],
        });
        assert!(matches!(outcome, RouteOutcome::Ignore));
    }


    #[test]
    fn media_child_job_state_carries_advisories_into_done() {
        // Invariant #2: a degraded advisory flows into Done.advisories.
        let advisory = Advisory {
            scope: Scope::File,
            severity: Severity::ShippedDegraded,
            item: Some("clip.mov".into()),
            what: "shipped unoptimized".into(),
            action: Action::None,
        };
        let state = media_child_job_state(vec![advisory]);
        match state {
            crate::tasks::TaskState::Succeeded { advisories, .. } => {
                assert_eq!(advisories.len(), 1);
            }
            other => panic!("expected Succeeded with advisories, got {other:?}"),
        }
    }

    #[test]
    fn media_child_job_state_blocking_flips_to_failed() {
        // Invariant #1: a Blocking advisory makes the media child Job Failed,
        // via the TaskState::done smart constructor (no re-decision in the ACL).
        let blocking = Advisory {
            scope: Scope::Account,
            severity: Severity::Blocking,
            item: None,
            what: "disk full".into(),
            action: Action::None,
        };
        let state = media_child_job_state(vec![blocking]);
        assert!(matches!(state, crate::tasks::TaskState::Failed { .. }));
    }

    // =========================================================================
    // Task 2.2: symlink-skip → L1 advisory (TDD)
    // =========================================================================

    #[test]
    fn symlink_skip_advisory_zero_returns_none() {
        // Zero skipped symlinks → no advisory (clean build, no hairline dot).
        assert!(make_symlink_skip_advisory(0).is_none());
    }

    #[test]
    fn symlink_skip_advisory_one_produces_needs_action() {
        // A single skipped symlink should produce a NeedsAction advisory so the
        // L1 hairline dot surfaces it without a toast.
        let event = make_symlink_skip_advisory(1).expect("expected Some for count=1");
        match event {
            PipelineEvent::BackgroundProgress {
                task,
                completed,
                advisories,
                ..
            } => {
                assert_eq!(task, "assets");
                assert!(completed, "advisory event must be completed=true");
                assert_eq!(advisories.len(), 1);
                let adv = &advisories[0];
                assert_eq!(adv.severity, Severity::NeedsAction);
                assert_eq!(adv.scope, Scope::Config);
                assert!(adv.what.contains("1"), "message must mention count");
                assert!(adv.item.is_none(), "build-wide advisory has no item");
            }
            other => panic!("expected BackgroundProgress, got {other:?}"),
        }
    }

    #[test]
    fn symlink_skip_advisory_many_produces_needs_action() {
        // Multiple skipped symlinks → advisory with count in the message.
        let event = make_symlink_skip_advisory(3).expect("expected Some for count=3");
        match event {
            PipelineEvent::BackgroundProgress { advisories, .. } => {
                assert_eq!(advisories.len(), 1);
                assert!(advisories[0].what.contains("3"), "message must mention count=3");
                assert_eq!(advisories[0].severity, Severity::NeedsAction);
            }
            other => panic!("expected BackgroundProgress, got {other:?}"),
        }
    }

    // =========================================================================
    // config schema_version ahead → L1 advisory (preview's twin of the
    // publish-time refusal in deploy/push.rs)
    // =========================================================================

    #[test]
    fn config_version_ahead_advisory_is_needs_action_not_blocking() {
        // NeedsAction, not Blocking: the build still renders (with the raw,
        // unmigrated table) — this is a notice, not a build failure. Publish
        // is the one path that refuses outright.
        let event = make_config_version_ahead_advisory(9).expect("expected Some");
        match event {
            PipelineEvent::BackgroundProgress { task, completed, advisories, .. } => {
                assert_eq!(task, "config");
                assert!(completed);
                assert_eq!(advisories.len(), 1);
                let adv = &advisories[0];
                assert_eq!(adv.severity, Severity::NeedsAction);
                assert_eq!(adv.scope, Scope::Config);
                assert!(adv.item.is_none(), "build-wide advisory has no item");
                assert!(adv.what.contains('9'), "message must name the found version: {}", adv.what);
            }
            other => panic!("expected BackgroundProgress, got {other:?}"),
        }
    }

    // I4 pluralization fix: "1 symlink(s)" → "1 symlink", "3 symlinks(s)" → "3 symlinks"
    #[test]
    fn symlink_skip_advisory_one_uses_singular_noun_no_parens() {
        // I4: the message must NOT contain "(s)" — that produces "1 symlink(s)".
        let event = make_symlink_skip_advisory(1).expect("expected Some for count=1");
        match event {
            PipelineEvent::BackgroundProgress { advisories, .. } => {
                let msg = &advisories[0].what;
                assert!(
                    !msg.contains("(s)"),
                    "message must not contain '(s)' (pluralization bug), got: {msg}",
                );
                assert!(
                    msg.starts_with("1 symlink "),
                    "count=1 message must start with '1 symlink ', got: {msg}",
                );
            }
            other => panic!("expected BackgroundProgress, got {other:?}"),
        }
    }

    #[test]
    fn symlink_skip_advisory_many_uses_plural_noun_no_parens() {
        // I4: count=3 → "3 symlinks skipped — …" (no "(s)" suffix).
        let event = make_symlink_skip_advisory(3).expect("expected Some for count=3");
        match event {
            PipelineEvent::BackgroundProgress { advisories, .. } => {
                let msg = &advisories[0].what;
                assert!(
                    !msg.contains("(s)"),
                    "message must not contain '(s)' (pluralization bug), got: {msg}",
                );
                assert!(
                    msg.starts_with("3 symlinks "),
                    "count=3 message must start with '3 symlinks ', got: {msg}",
                );
            }
            other => panic!("expected BackgroundProgress, got {other:?}"),
        }
    }
}
