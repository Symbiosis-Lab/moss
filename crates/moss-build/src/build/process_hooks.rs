//! The build's plugin-process seam: the one spawn.
//!
//! Split out of `build.rs` when `spawn_process_hooks` stopped needing an
//! `AppHandle` (#1019). It once also ran the login-needed scan and forked its
//! failure report on `shell_attached`; both folded into the manager (the scan
//! reports `PipelineEvent::PluginNeedsConnection`, ADR-076) and into
//! `log_warn_problem!`, which reaches the headless logger on the CLI and the
//! log file in the app.

use super::*;

/// Execute process hooks for installed plugins (non-blocking).
///
/// This function spawns a background task and returns immediately.
/// Process hooks run pre-processing before generation (e.g., fetching comments from server).
/// Errors are logged but do not abort.
///
/// Returns a handle that can be awaited if you need to wait for completion.
pub fn spawn_process_hooks(
    folder_path: &str,
    plugins: Arc<crate::plugins::manager::ManagerCache>,
    spawner: &dyn crate::build::ports::spawner::Spawner,
    project_info: ProjectInfo,
) -> crate::build::ports::spawner::Joining {
    let folder_path = folder_path.to_string();

    // The spawner runs it on moss's ONE async runtime; a second runtime would
    // corrupt the heap, which is why this is a port and not a `tokio::spawn`.
    spawner.spawn(Box::pin(async move {
        // This is the build/preview/rebuild path — not a deliberate onboarding
        // gesture, so the trigger is Background (via `for_build`): plugin tasks
        // route to the quiet Workspace surface, NOT the hairline.
        let context = ProcessContext::for_build(folder_path, project_info);

        // Counted AND logged: a process hook that fails means plugin content is
        // missing from this build, so `--strict` must fail on it — a plugin-
        // bearing `moss build` once dropped the content, printed "Build
        // complete" and exited 0, the silent outcome ADR-050 exists to prevent.
        // Resolving the manager is part of the run: an engine that cannot
        // start is a failed process hook, reported the same way.
        let run = async {
            plugins
                .get_or_create(&context.project_path)
                .map_err(|e| format!("plugins could not start: {e}"))?
                .execute_process(&context)
                .await
        };
        if let Err(e) = run.await {
            crate::build::cli_output::log_warn_problem!(target: "plugin", "process hook failed: {e}");
        }
    }))
}
