//! Operational modes over the built site — long-running processes a host
//! starts and owns, as opposed to the one-shot pipeline under [`crate::build`].
//!
//! NORTH-STAR charters this family: `ops/serve` (the preview server, crossed
//! at S1 of the 2026-08-28 relocation plan, ADR-067) and `ops/watch` (the
//! debouncer driver, crossed at slice W1 of the same plan).

pub mod serve;
pub mod watch;

use std::sync::Arc;

use crate::build::cli_output::{finish_cli_build, install_headless_logger, preflight_cli_build};
use crate::build::{run_pipeline, BuildTrigger, HostPorts, HostStore, PipelineConfig, PluginMode};
use crate::cli_eprintln;
use crate::vault::paths::VaultRoot;

/// Start the file watch for `folder` with the given rebuild plugin mode,
/// returning the shutdown sender. Host-provided because the watch host glue
/// differs per binary: the app's arm
/// (`build_shell/watch.rs::start_file_watching_headless`) layers the app-side
/// sweep over [`watch::headless::start`]; moss-cli's is that construction
/// alone. The mode is the driver's to decide, so neither host derives it.
pub type WatchStarter = Box<
    dyn FnOnce(
            String,
            PluginMode,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = tokio::sync::oneshot::Sender<()>> + Send>,
        > + Send,
>;

impl HostPorts {
    /// The host with no shell — what `moss-cli build` and the app binary's
    /// headless `moss build` both run the pipeline with, so the two cannot
    /// drift (#1154 folded the twin they used to be). It lives here, beside
    /// its consumer [`HeadlessBuildRun`], because `build/` must not reach
    /// into `ops/`; the struct itself is a port shape. Every `Option` is
    /// `None`; Tier-2 events, seal announcements, the services reporter and
    /// the plugin managers all publish to the HTTP carrier's SSE bus, which
    /// with no `--serve` is a no-op plus the log lines and under `--serve` is
    /// exactly what an attached browser hears. The spawner is plain tokio:
    /// a headless process blocks on its own runtime. `launch_server` stays
    /// `None` because the headless driver (`ops::run_headless_build`) installs
    /// the serve closure itself, to hold the shutdown sender for ctrl-C.
    ///
    /// `store` is the one answer the two binaries give differently: the app
    /// binary rewrites a legacy `.moss/` on disk, moss-cli migrates in memory.
    pub fn headless(folder_path: &str, store: Arc<dyn HostStore>) -> HostPorts {
        let carrier: Arc<dyn crate::build::ports::reporter::BuildReporter> =
            Arc::new(serve::events::CarrierReporter);
        let mut services = crate::types::services::BuildServices::headless_for(folder_path);
        services.reporter = carrier.clone();
        HostPorts {
            shell_attached: false,
            site_dir: None,
            metadata_dedup: None,
            scan_events: None,
            events: carrier.clone(),
            announcer: Arc::new(serve::events::CarrierAnnouncer),
            spawner: Arc::new(crate::build::ports::spawner::TokioSpawner),
            store,
            plugins: Arc::new(crate::plugins::manager::ManagerCache::headless(carrier)),
            server_diff: None,
            launch_server: None,
            services,
        }
    }
}

/// The `moss build` flags, parsed once for both binaries. Neither argument
/// parser holds a serve or plugin policy of its own: `moss-cli build` and the
/// app binary's `moss build` both call [`BuildArgs::parse`] on everything
/// after the verb and hand the result to [`HeadlessBuildRun`]. Before this
/// the two parsers were a six-line `contains` twin edited in step by hand
/// (ADR-077 added `--allow-plugins` to both that way).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BuildFlags {
    /// `--serve`: start the preview server and block until ctrl-C.
    pub serve: bool,
    /// `--watch`: also rebuild on file change (requires `serve` — refresh
    /// events have nowhere to go without the SSE carrier).
    pub watch: bool,
    /// `--strict`: exit 1 if the build reported any problems.
    ///
    /// moss prints warnings to stderr and then exits 0, which is right for a
    /// person watching the terminal and useless to an agent running
    /// unattended — it has no signal short of diffing stderr. `--strict` is
    /// that signal (Hugo's `--panicOnWarning`).
    ///
    /// **Opt-in, and it must stay opt-in.** A build that is green today has
    /// to stay green tomorrow unless the user asked for the stricter
    /// reading; the flag never changes what moss *writes*, only how it
    /// reports on what it wrote.
    pub strict: bool,
    /// `--wait-plugins`: force `PluginMode::Blocking` when plugins run. Also forced by `serve && !watch`: detaching the process hook is
    /// only safe if something later renders what it writes — under `--serve`
    /// alone no watcher will, so the site would serve pre-import content
    /// forever. A Config decision at the entry point, not a fork inside the
    /// pipeline (ADR-010).
    pub wait_plugins: bool,
    /// `--no-plugins`: skip plugin hooks in the initial build and the watch
    /// rebuilds alike. Otherwise the initial build runs them (`NonBlocking`,
    /// or `Blocking` when waiting) and a watch rebuild replays cached slots
    /// without re-running them (`SlotsOnly`, the GUI rebuild rule). Derived
    /// once, below, for both binaries — since the manager crossed (ADR-076)
    /// neither has a plugin policy of its own.
    pub no_plugins: bool,
    /// `--allow-plugins`: this command is the consent for the plugins the
    /// folder carries. Without it a plugin the app has never been told to
    /// allow is refused, as it is in the app before the user clicks Allow
    /// (ADR-077). Meaningless under `no_plugins`.
    pub allow_plugins: bool,
    /// `--site-url=<url>`: the URL baked into og:image, canonical links,
    /// sitemap and RSS, for one-off staging builds or a site not deployed via
    /// moss. Its precedence over `.moss/state.toml [deployment]` and the
    /// `MOSS_SITE_URL` variable is `build/site_url.rs`'s.
    pub site_url_override: Option<String>,
}

/// A parsed `moss build` command line — the folder and its flags, from the
/// one parser both binaries read.
///
/// Same shape as [`crate::cli::deploy::DeployArgs`], and for the same reason:
/// an unrecognised option must be refused rather than dropped. Ignoring it
/// silently meant `moss build site --allow-plugin` — one character short —
/// built with consent DENIED and said nothing, which is exactly the command a
/// user sent here by a refusal types next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArgs {
    /// The folder exactly as the user typed it. Each binary resolves it
    /// itself; an error naming a resolved absolute path names something the
    /// user never wrote.
    pub folder: String,
    pub flags: BuildFlags,
}

impl BuildArgs {
    /// `args` is everything after `moss build`. `Err` is the whole text to
    /// print on stderr before exiting 1 — the shared command table's usage for
    /// a missing folder, an `error:` line for anything the command does not
    /// accept.
    ///
    /// The folder is a positional, so it cannot be recovered from a flag:
    /// `moss build --allow-plugins` with no folder used to resolve the flag
    /// itself as the folder in the app binary and print usage in the open one.
    pub fn parse(args: &[String]) -> Result<BuildArgs, String> {
        let mut folder: Option<&str> = None;
        let mut flags = BuildFlags::default();
        for arg in args {
            match arg.as_str() {
                "--serve" => flags.serve = true,
                "--watch" => flags.watch = true,
                "--strict" => flags.strict = true,
                "--wait-plugins" => flags.wait_plugins = true,
                "--no-plugins" => flags.no_plugins = true,
                "--allow-plugins" => flags.allow_plugins = true,
                a if a.starts_with("--site-url=") => {
                    flags.site_url_override = Some(a["--site-url=".len()..].to_string())
                }
                a if a.starts_with('-') => {
                    return Err(format!("error: unknown option '{}'\n{}", a, usage()))
                }
                a => {
                    if folder.is_some() {
                        return Err("error: build takes one folder".to_string());
                    }
                    folder = Some(a);
                }
            }
        }
        match folder {
            Some(folder) => Ok(BuildArgs { folder: folder.to_string(), flags }),
            None => Err(usage()),
        }
    }
}

/// The usage line both binaries print, from the shared command table, so it
/// cannot drift from `moss build --help` or from `moss describe`.
fn usage() -> String {
    crate::cli::commands::help_for("build").expect("build is in the command table")
}

/// Everything [`run_headless_build`] needs, resolved by the host's argument
/// parser. Both CLI entry points — `moss-cli build` and the app binary's
/// headless `moss build` interception — build one of these and nothing else;
/// the ~130-line build-run/wait/exit body they used to mirror
/// (`startup/headless.rs::run_cli_build` and its documented moss-cli twin)
/// lives once, below (slice C1's headline deletion).
pub struct HeadlessBuildRun {
    /// The vault to build.
    pub folder_path: String,
    /// Everything after the folder on the command line.
    pub flags: BuildFlags,
    /// See [`watch::headless::HostPortsFactory`] — called once for the
    /// initial build (after the folder session is registered, which is why
    /// this is a factory and not a value) and once per watch rebuild.
    pub host_ports: watch::headless::HostPortsFactory,
    /// See [`WatchStarter`].
    pub start_watch: WatchStarter,
}

/// Run a headless CLI build — the one driver behind both `moss-cli build`
/// and the app binary's `moss build`. Never returns.
///
/// Owns the whole process shape: logger, preflight, the per-folder session
/// (the stage-write lock every concurrent-build path reaches for), one plain
/// tokio runtime, the pipeline call, the problem summary, and — under
/// `--serve` — the watch start and the ctrl-C wait that shuts the server
/// down. The serve arm wraps the host's `launch_server` port with a closure
/// over [`serve::start_server_headless`] so the driver holds the server's
/// shutdown sender; the pipeline still owns the serve-dir cell handoff (it
/// passes its own cell to the closure — the contract `ops/serve.rs`
/// documents: the build must own the cell or `--serve` 404s forever).
pub fn run_headless_build(run: HeadlessBuildRun) -> ! {
    install_headless_logger();
    preflight_cli_build(&run.folder_path);

    // Register the session before any build work: the pipeline, the seal tail
    // and the watcher's admission probe all reach for it to take the
    // per-folder stage-write lock. `--serve --watch` — the one CLI shape with
    // concurrent builds — is exactly the shape that needs it (2026-08-24).
    // Dropping the binding does not remove it from the registry.
    let _session = crate::system::folder_session::register_session(&run.folder_path);

    // The admission verdict, before anything can load a plugin. Set-once for
    // the process, so it is installed here rather than by each host.
    if !run.flags.no_plugins {
        crate::plugins::install::registry_client::enforce::install_headless(run.flags.allow_plugins);
    }

    let wait_plugins = run.flags.wait_plugins || (run.flags.serve && !run.flags.watch);
    let (plugins, watch_plugins) = match run.flags.no_plugins {
        true => (PluginMode::Skip, PluginMode::Skip),
        false if wait_plugins => (PluginMode::Blocking, PluginMode::SlotsOnly),
        false => (PluginMode::NonBlocking, PluginMode::SlotsOnly),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the tokio runtime for a headless build");
    let code = runtime.block_on(async move {
        // The driver holds the server's shutdown sender (filled by the launch
        // closure below) so ctrl-C can shut the server down deliberately —
        // the graceful replacement for the `mem::forget` this path carried
        // when its only exit was `std::process::exit`.
        let server_shutdown: Arc<
            std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        > = Arc::default();

        let mut host = (run.host_ports)(&run.folder_path);
        if run.flags.serve {
            let slot = server_shutdown.clone();
            // The same registry this build fills, so the server can answer for
            // a variant that is still encoding (ADR-013).
            let assets = host.services.assets.clone();
            host.launch_server = Some(Arc::new(move |moss_dir, cell| {
                let slot = slot.clone();
                let assets = assets.clone();
                Box::pin(async move {
                    let (port, shutdown_tx) =
                        serve::start_server_headless(&moss_dir, cell, assets).await?;
                    *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(shutdown_tx);
                    Ok(port)
                })
            }));
        }

        let result = run_pipeline(PipelineConfig {
            root: VaultRoot::resolve(std::path::Path::new(&run.folder_path)),
            progress: crate::build::stdout_sink(),
            plugins,
            watch: run.flags.watch,
            start_server: run.flags.serve,
            host,
            trigger: BuildTrigger::Full,
            // A serving process stays alive (it parks on ctrl-C below), so
            // only the plain build seals inline / prewarms synchronously.
            exits_after_build: !run.flags.serve,
            site_url_override: run.flags.site_url_override,
            server_port: None,
            live_port: None,
            admission_epoch: None,
        })
        .await;

        match result {
            Ok(msg) => {
                let problems = finish_cli_build(&msg, &run.folder_path, run.flags.strict).await;

                // Held until shutdown: dropping the sender stops the watch.
                // Rebuild events reach an attached browser over the SSE
                // carrier (`ops/serve/events.rs`).
                let _watch_shutdown = if run.flags.serve && run.flags.watch {
                    cli_eprintln!("Watching for file changes (Ctrl+C to stop)");
                    Some((run.start_watch)(run.folder_path.clone(), watch_plugins).await)
                } else {
                    None
                };

                if run.flags.serve {
                    cli_eprintln!("Press Ctrl+C to stop the server");
                    let _ = tokio::signal::ctrl_c().await;
                    cli_eprintln!("Stopping...");
                    if let Some(tx) = server_shutdown
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        let _ = tx.send(());
                    }
                    cli_eprintln!("Stopped");
                } else if run.flags.watch {
                    cli_eprintln!("--watch was requested but --serve is required to receive refresh events; ignoring --watch");
                }

                if run.flags.strict && problems > 0 {
                    1
                } else {
                    0
                }
            }
            Err(e) => {
                cli_eprintln!("Build failed: {}", e);
                1
            }
        }
    });
    std::process::exit(code);
}
