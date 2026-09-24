//! Everything the build says to whoever asked for it — the port that replaced
//! `ProgressSink`, and now also the three signals that are not progress.
//!
//! The pipeline reports what happened; the caller decides what that means.
//! The desktop app forwards to the loading screen and the typed event bus, the
//! CLI prints to stderr, tests discard. Nothing here names tauri, which is the
//! point: `build/` is scheduled to become the open `moss-build` crate (ADR-050),
//! and `check-crate-dag.mjs` rule 2 forbids a path to tauri from it.
//!
//! The tauri-backed implementation lives app-side in `crate::events`.

use crate::build::cli_output::{cli_eprintln, cli_warn};

use crate::build::progress::PipelineEvent;

/// How a cloud download is going, as one producer sees it.
///
/// A struct rather than six positional parameters because the *other* producer
/// (`build_shell::watch::sweep`, which is not a build) fills in the same fields,
/// and two call sites that must not drift are easier to compare when the field
/// names are written down.
pub struct CloudSync<'a> {
    /// Which folder these counts describe. The event is app-wide, and a tick
    /// that lands after the user switched folders would otherwise report one
    /// folder's counts on another folder's panel.
    pub folder: &'a str,
    pub phase: &'a str,
    pub provider: Option<crate::build::cloud_provider::CloudProvider>,
    pub total: usize,
    pub remaining: usize,
    /// How many of `remaining` the render actually waits on — the subset the
    /// full-window waiting screen is keyed on.
    ///
    /// `None` from the build, which does not compute it: the gate verdict it
    /// emits (`home_waiting`/`home_ready`) already answers the same question,
    /// and a listener that sees no count falls back to `remaining`, which is
    /// what every producer meant before this field existed. The supervisor's
    /// sweep always fills it in (moss#1077).
    pub blocking: Option<usize>,
    /// Named files the provider will not hand over, base names only. Empty for
    /// every phase but `unavailable`.
    pub unavailable: &'a [String],
}

/// Sink for everything the build wants to say.
///
/// `Send + Sync` is load-bearing rather than decorative: the reporter is held
/// in `PipelineConfig` and crosses `spawn_blocking`, so it is shared across
/// threads by construction.
///
/// ## Why the three control signals live here and not on a port of their own
///
/// `stage_ready`, `cloud_sync` and `download_progress` are not `PipelineEvent`s
/// — they are untyped, separately-listened-to signals, and the first is a
/// *control* call that sets the root the preview server serves rather than a
/// description of anything. That argues for a second trait, and a second trait
/// is what this was until NORTH-STAR was re-read: the moss-build seam is
/// **five ports**, `BuildReporter` is specified as "progress + `stage_ready`",
/// and a **fourth channel-shaped port trips the pre-approved abort threshold**
/// — the signal that the crate line is drawn too low. Splitting them would have
/// bought tidier types at the cost of the falsifier that is supposed to catch a
/// bad boundary.
///
/// **That cut both ways, and the second edge is now settled.** Counting *ports*
/// meant folding a concern onto an existing trait moved no count — any future
/// shell signal could arrive as one more default method and the detector would
/// never fire. Four channel-shaped concerns cross this seam (progress,
/// `stage_ready`, `cloud_sync`, `download_progress`) and all four are methods on
/// this one trait, so the fold was not hypothetical. ADR-058 (accepted
/// 2026-08-18) replaces the trait count with a width count — ratchet row (o)
/// `seam_surface` — which a fold moves exactly as much as a new port does. Add a
/// method here and the tripwire goes red.
pub trait BuildReporter: Send + Sync {
    /// Report one pipeline event.
    fn report(&self, event: &PipelineEvent);

    /// A servable site now exists at `site_path`; serve from there.
    fn stage_ready(&self, _site_path: &std::path::Path) {}

    /// Report cloud download state to the waiting screen.
    fn cloud_sync(&self, _status: &CloudSync<'_>) {}

    /// Report bytes fetched of a build-time binary dependency (JupyterLite).
    fn download_progress(&self, _binary: &str, _bytes: u64, _total: Option<u64>) {}

    /// Whether a **window** is listening for the three signals above.
    ///
    /// Not "is anyone listening at all": [`StdoutReporter`] answers false here
    /// and true to `is_terminal()`, because a terminal is not a window and none
    /// of these three signals has a CLI rendering. Three near-synonym
    /// predicates on one trait is one too many to tell apart by feel, so:
    /// `wants_tier1` = is there a progress *channel*; `is_terminal` = is stderr
    /// the surface; `shell_listening` = is there a *window*.
    ///
    /// False by default, and the two call sites that read it are not being
    /// defensive: both would otherwise do real work — a `stat` per evicted
    /// file, a callback per 64KB chunk — whose only consumer is a window that
    /// does not exist. Before this was a port they asked `app.is_none()`, which
    /// answered the same question. A port that replaces an `Option` has to
    /// carry the predicate the `Option` was answering, or it quietly changes
    /// what the build does.
    fn shell_listening(&self) -> bool {
        false
    }

    /// Whether Tier-1 (loading-screen) progress has anywhere to go.
    ///
    /// False for stdout and null, and that is not an oversight. Tier-1 events
    /// describe fine-grained pipeline steps for a GUI progress bar; the CLI
    /// prints its own coarse build progress at the entry point instead, and
    /// before this port existed the CLI simply had no Tier-1 channel, so
    /// `send_progress` was a no-op there. Returning true by default would start
    /// printing every internal step to stderr — a change in CLI output dressed
    /// up as a refactor.
    fn wants_tier1(&self) -> bool {
        false
    }

    /// Whether this reporter writes to a terminal, i.e. whether the build is
    /// running headless. Replaces `matches!(config.progress, Stdout)`.
    fn is_terminal(&self) -> bool {
        false
    }
}

/// Discard everything — tests, GUI rebuilds with no progress channel, and
/// [`discarding`], which is what answers `shell_listening()` for code holding
/// no `BuildServices` at all.
pub struct NullReporter;

impl BuildReporter for NullReporter {
    fn report(&self, _event: &PipelineEvent) {}
}

/// There is no GUI: nothing to report to, and the terminal is the surface.
///
/// This is `BuildServices.reporter` on a headless build, and the difference
/// from [`NullReporter`] is the *answer to a different question*. Both discard
/// events. Only this one says `is_terminal()`, which is what the pipeline's
/// `eprintln!` sites read to decide whether to speak — a GUI rebuild with no
/// progress channel is also `NullReporter`, and it must stay silent.
///
/// It does not print, because on a headless build the CLI's own coarse
/// progress already goes to stderr through [`StdoutReporter`] as the
/// *progress sink*. Printing here too would double every line.
pub struct HeadlessReporter;

impl BuildReporter for HeadlessReporter {
    fn report(&self, _event: &PipelineEvent) {}

    fn is_terminal(&self) -> bool {
        true
    }
}

/// A reporter for code holding an `Option<&BuildServices>`: when there are no
/// services at all, there is nobody to report to.
///
/// `services.map_or(reporter::discarding(), |s| s.reporter.as_ref())`.
pub fn discarding() -> &'static dyn BuildReporter {
    static DISCARD: NullReporter = NullReporter;
    &DISCARD
}

/// [`discarding`] for the closures that need to own theirs.
pub fn discard_owned() -> std::sync::Arc<dyn BuildReporter> {
    std::sync::Arc::new(NullReporter)
}

/// Print human-readable progress to stderr — CLI mode.
pub struct StdoutReporter;

impl BuildReporter for StdoutReporter {
    fn is_terminal(&self) -> bool {
        true
    }

    fn report(&self, event: &PipelineEvent) {
        match event {
            PipelineEvent::Progress {
                step,
                message,
                percentage,
                completed,
                ..
            } => {
                if *completed {
                    cli_eprintln!("[done] {}", message);
                } else {
                    cli_eprintln!("[{:>3}%] {}: {}", percentage, step, message);
                }
            }
            PipelineEvent::BackgroundProgress {
                task,
                current,
                total,
                completed,
                advisories,
                ..
            } => {
                for line in background_progress_lines(task, *current, *total, *completed, advisories) {
                    cli_eprintln!("{}", line);
                }
            }
            PipelineEvent::VideoProgress {
                filename,
                phase,
                current_video,
                total_videos,
                ..
            } => {
                cli_eprintln!(
                    "[video {}/{}] {} - {}",
                    current_video, total_videos, filename, phase
                );
            }
            PipelineEvent::BuildComplete {
                videos_converted,
                total_time_ms,
                skipped_symlinks,
            } => {
                let secs = *total_time_ms as f64 / 1000.0;
                if *skipped_symlinks > 0 {
                    cli_eprintln!(
                        "[complete] Build finished in {:.1}s ({} videos converted, {} symlinks skipped)",
                        secs, videos_converted, skipped_symlinks
                    );
                } else {
                    cli_eprintln!(
                        "[complete] Build finished in {:.1}s ({} videos converted)",
                        secs, videos_converted
                    );
                }
            }
            PipelineEvent::AssetReady { path, asset_type } => {
                cli_eprintln!("[asset] {} ready ({})", path, asset_type);
            }
            PipelineEvent::AssetsSettled { changed } => {
                cli_eprintln!("[assets] {} variant(s) settled", changed.len());
            }
            PipelineEvent::FileChanged(_) => {
                cli_eprintln!("[watch] File changed, rebuilding...");
            }
            PipelineEvent::Error { phase, message } => {
                cli_eprintln!("[error] {}: {}", phase, message);
            }
            PipelineEvent::PreviewServerFailed { message } => {
                cli_eprintln!("[error] preview server failed: {}", message);
            }
            PipelineEvent::PluginProgress(p) => {
                let detail = p.message.as_deref().map(|m| format!(": {m}")).unwrap_or_default();
                if p.completed {
                    cli_eprintln!("[plugin {}] {} {}{}", p.plugin_name, p.hook_name, p.phase, detail);
                } else {
                    cli_eprintln!("[plugin {}] {} ({}/{}){}", p.plugin_name, p.phase, p.current, p.total, detail);
                }
            }
            // `cli_warn!`, not `cli_eprintln!`: this line says content is
            // missing from the build, so it must also COUNT — printed through
            // `cli_eprintln!` it would leave `--strict` green.
            PipelineEvent::PluginNeedsConnection { plugin, .. } => {
                cli_warn!(
                    "{plugin} is not connected for this folder; its content will be \
                     skipped. Connect it in moss, or re-run with --no-plugins."
                );
            }
        }
    }
}

/// What [`StdoutReporter`] prints for one `BackgroundProgress` tick — zero,
/// one or several lines. Pure so the rendering choice is unit-testable
/// without capturing real stderr (mirrors [`problem_summary_line`] in
/// `cli_output.rs`).
///
/// `current == 0 && total == 0` is the shape a build-wide check's own tick
/// takes (`advisory_event` / `clear_tick` in `build/progress.rs` — symlink-
/// skip, config-version-ahead, duplicate-uid): the task never did any
/// "progress" on this tick, so "complete (0/0)" would be a routing artifact,
/// not a fact about the build. Silent when there is nothing to say — the
/// clear tick, which a build-wide check now emits every time it runs clean,
/// where before it emitted nothing at all — and one `[advisory]` line per
/// advisory otherwise, naming what the check actually found instead of a
/// fake completion count.
fn background_progress_lines(
    task: &str,
    current: u32,
    total: u32,
    completed: bool,
    advisories: &[crate::advisory::Advisory],
) -> Vec<String> {
    if current == 0 && total == 0 {
        return advisories
            .iter()
            .map(|a| format!("[advisory] {task}: {}", a.what))
            .collect();
    }
    vec![if completed {
        format!("[done] {task} complete ({current}/{total})")
    } else {
        format!("[bg] {task} ({current}/{total})")
    }]
}

#[cfg(test)]
mod background_progress_lines_tests {
    use super::background_progress_lines;
    use crate::advisory::{Action, Advisory, Scope, Severity};

    fn advisory(what: &str) -> Advisory {
        Advisory {
            scope: Scope::Config,
            severity: Severity::NeedsAction,
            item: None,
            what: what.into(),
            action: Action::None,
        }
    }

    #[test]
    fn a_clear_tick_zero_zero_no_advisories_prints_nothing() {
        // The new shape a clean build-wide check emits every build — must
        // NOT print a fake "complete (0/0)" line.
        assert!(background_progress_lines("assets", 0, 0, true, &[]).is_empty());
    }

    #[test]
    fn a_zero_zero_tick_with_one_advisory_prints_it_not_a_completion_count() {
        let lines = background_progress_lines("config", 0, 0, true, &[advisory("schema is 2 versions ahead")]);
        assert_eq!(lines, vec!["[advisory] config: schema is 2 versions ahead".to_string()]);
    }

    #[test]
    fn a_zero_zero_tick_with_several_advisories_prints_one_line_each() {
        let lines = background_progress_lines(
            "markdown",
            0,
            0,
            true,
            &[advisory("dup 1"), advisory("dup 2")],
        );
        assert_eq!(
            lines,
            vec!["[advisory] markdown: dup 1".to_string(), "[advisory] markdown: dup 2".to_string()]
        );
    }

    #[test]
    fn real_progress_at_completion_still_prints_the_done_line() {
        assert_eq!(
            background_progress_lines("images", 12, 12, true, &[]),
            vec!["[done] images complete (12/12)".to_string()]
        );
    }

    #[test]
    fn real_progress_in_flight_prints_the_bg_line() {
        assert_eq!(
            background_progress_lines("videos", 3, 6, false, &[]),
            vec!["[bg] videos (3/6)".to_string()]
        );
    }
}
