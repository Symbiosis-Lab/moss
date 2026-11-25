//! The build's plugin-process seam: the one spawn.
//!
//! Split out of `build.rs` when `spawn_process_hooks` stopped needing an
//! `AppHandle`. It once also ran the login-needed scan and forked its
//! failure report on `shell_attached`; both folded into the manager (the scan
//! reports `PipelineEvent::PluginNeedsConnection`) and into
//! `log_warn_problem!`, which reaches the headless logger on the CLI and the
//! log file in the app.

use super::*;

/// Run the process hooks of the folder's plugins on the host's runtime and
/// return at once; errors are logged, never fatal. `trigger` is `Background`
/// for a build, which reaches no network, and the person's trigger when the
/// app opens the folder, which is its import.
pub fn spawn_process_hooks(
    folder_path: &str,
    plugins: Arc<crate::plugins::manager::ManagerCache>,
    spawner: &dyn crate::build::ports::spawner::Spawner,
    project_info: ProjectInfo,
    trigger: crate::plugins::TriggerContext,
) -> crate::build::ports::spawner::Joining {
    let folder_path = folder_path.to_string();

    // The spawner runs it on moss's ONE async runtime; a second runtime would
    // corrupt the heap, which is why this is a port and not a `tokio::spawn`.
    spawner.spawn(Box::pin(async move {
        let context = ProcessContext::new(folder_path, project_info, trigger);

        // Counted AND logged: a process hook that fails means plugin content is
        // missing from this build, so `--strict` must fail on it — a plugin-
        // bearing `moss build` once dropped the content, printed "Build
        // complete" and exited 0, the silent outcome this guard exists to prevent.
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

const SYNC_USAGE: &str = "Usage: moss sync <folder> [--allow-plugins]";

/// `moss sync`: the folder's process hooks once, as the person's run the app
/// starts when the folder opens or they press Sync, then exit without
/// building. A build reaches no network, so this is where a command-line user's
/// plugins fetch; the next build reads what they wrote.
pub fn sync(args: &[String]) -> i32 {
    use crate::plugins::install::registry_client::enforce;
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{SYNC_USAGE}\n\nRuns the process hooks of the plugins in <folder> once, as the moss app does when the folder opens or Sync is pressed (for example, importing new posts), then exits without building. Plugins that came with the folder run only once the folder is trusted in the moss app, or with --allow-plugins.");
        return 0;
    }
    let allow = args.iter().any(|a| a == "--allow-plugins");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--allow-plugins").collect();
    let [target] = rest[..] else {
        eprintln!("{SYNC_USAGE}");
        return 2;
    };
    let root = VaultRoot::resolve(target);
    let folder = root.as_str().to_string();
    if !Path::new(&folder).is_dir() {
        eprintln!("error: folder does not exist: {target}");
        return 1;
    }
    if let Err(message) = crate::cli::site_guard::guard_cli_open(&folder, "sync") {
        eprintln!("error: {message}");
        return 1;
    }
    crate::build::cli_output::install_headless_logger();
    enforce::install_headless_for_running_app(allow);
    let waiting = enforce::waiting_on_trust(Path::new(&folder));
    if !allow && !waiting.is_empty() {
        eprintln!("error: this folder is not trusted, so {} cannot run: pass --allow-plugins, or open the folder in the moss app and trust it", waiting.join(", "));
        return 1;
    }
    let owner = crate::build::cloud_readiness::storage::OperationOwner::Host;
    let structure = match scan::scan::scan_folder_with_dedup_emit_with_error(&folder, None, None, false, owner) {
        Ok(structure) => structure,
        Err(e) => {
            eprintln!("error: failed to scan folder: {e}");
            return 1;
        }
    };
    let context = ProcessContext::new(folder.clone(), ProjectInfo::from_structure(&structure, &root), crate::plugins::TriggerContext::SettingsManual);
    let host = crate::cli::host::cli_host_ports(&folder);
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("error: failed to start runtime: {e}");
            return 1;
        }
    };
    let run = runtime.block_on(async {
        host.plugins.get_or_create(&folder).map_err(|e| format!("plugins could not start: {e}"))?.execute_process(&context).await.map_err(|e| format!("process hook failed: {e}"))
    });
    match run {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
