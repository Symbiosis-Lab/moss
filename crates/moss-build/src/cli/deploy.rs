//! `moss deploy` — publish a folder from a terminal, in either binary.
//!
//! [`run`] is the open binary's whole command; the app enters at [`publish`]
//! from `startup::headless::intercept`, having already parsed the same
//! arguments. What each supplies is a `host_ports` factory and, in the app's
//! case, the Finder stamp that cannot cross.
//!
//! Three routes, and the folder decides which:
//! [`crate::deploy::route::DeployRoute`] reads the answer once and both
//! binaries act on it. A **prebuilt** folder — one whose site was produced by
//! another tool (Quire, Hugo, Jekyll, Astro), named by `--prebuilt=<dir>` or by
//! `[deployment].prebuilt_output` — is uploaded as-is. A folder whose
//! `[hooks] deploy` names a **plugin** is built and then handed to that plugin.
//! Everything else takes the **moss-hosted** route, where moss builds the site
//! and publishes what it sealed, registering it first if this is the folder's
//! first publish.
//!
//! The hosted route arrived at C4f and is why this binary stopped saying
//! "building it needs the moss app"; first-publish registration crossed at
//! C4g, which is why `--site-id` stopped saying it too. The plugin route was
//! the last one to say it, and stopped at P2b — no route needs a window now,
//! which is why the app's `intercept` never returns.
//!
//! Plugins in the deploy build need consent, and `--allow-plugins` is the
//! consent here that it is for `moss build`: without it a plugin the app has
//! allowed runs and one nobody allowed is refused, so its content is missing
//! from what gets published. The flag used to be named by that refusal and
//! not accepted by this command, which left the reader a dead end; `moss build
//! --allow-plugins <folder>` first is still the way to see the decision before
//! it reaches a live site.
//!
//! Exit codes:
//!   0  published
//!   1  usage, needs setup, or the publish failed

use crate::deploy::progress::{DeployProgress, DeploySink};
use crate::deploy::route::DeployRoute;
use crate::deploy::{DeployReport, PushResult};
use crate::vault_root::VaultRoot;
use std::sync::Arc;

/// Prints each thing a publish says, once.
///
/// The upload stage is the only one that repeats: the hosted route's ticker
/// fires several times a second, and the prebuilt route emits one event per
/// uploaded file. A terminal is not a progress bar, so the stage announces
/// itself once and the rest is silence until the next stage. Every other
/// stage fires once already and is printed as it comes — de-duplicating on
/// anything coarser than the upload stage loses lines, because a first
/// publish says both "Registering site…" and "Building the site…" under
/// `Preparing` and the second is the one that takes minutes.
#[derive(Default)]
struct PrintingSink {
    upload_announced: std::sync::atomic::AtomicBool,
}

impl PrintingSink {
    /// The line this event prints, or `None` when the upload stage has
    /// already announced itself. Separate from the printing so a test can
    /// read the decision.
    fn line<'a>(&self, event: &'a DeployProgress) -> Option<&'a str> {
        if event.stage == crate::deploy::progress::DeployStage::Uploading
            && self
                .upload_announced
                .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        Some(&event.message)
    }
}

impl DeploySink for PrintingSink {
    fn deploy_progress(&self, event: DeployProgress) {
        if let Some(line) = self.line(&event) {
            eprintln!("→ {}", line);
        }
    }
}

fn printing() -> Arc<dyn DeploySink> {
    Arc::new(PrintingSink::default())
}

/// The `moss deploy` flags, parsed once for both binaries.
///
/// Same reason `moss build`'s live in [`crate::ops::BuildFlags`]: two argument
/// parsers holding one policy drift silently, and the two here are decisions
/// rather than spellings — `--prebuilt` picks the route, `--allow-plugins` is
/// consent for code the folder carries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeployFlags {
    /// `--prebuilt=<dir>`: a static site another tool produced, uploaded
    /// as-is. Resolved relative to the folder unless absolute; also settable
    /// as `.moss/state.toml [deployment].prebuilt_output`.
    pub prebuilt: Option<String>,
    /// `--site-id=<name>`: the site ID to register on a first publish, over
    /// the one derived from the folder name.
    pub site_id: Option<String>,
    /// `--allow-plugins`: this command is the consent for the plugins the
    /// folder carries, exactly as it is for `moss build`. Without
    /// it a plugin the app has never been told to allow is refused — which,
    /// on the route that publishes THROUGH a plugin, refuses the publish.
    pub allow_plugins: bool,
    /// `--overwrite-newer`: publish even though the live site was published
    /// from another copy after this folder's last publish, undoing that
    /// publish. Named for what it discards rather than `--force`, because
    /// the other publish refusals have no override and must not look as if
    /// this one lifts them.
    pub overwrite_newer: bool,
}

/// A parsed `moss deploy` command line.
///
/// [`Self::parse`] takes everything after `moss deploy` and is the only place
/// either binary reads a deploy argument. `Err` is the whole text to print on
/// stderr before exiting 1 — usage for a missing folder, an `error:` line for
/// anything the command does not accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployArgs {
    /// The folder exactly as the user typed it. Each binary resolves it
    /// itself; an error naming a resolved absolute path names something the
    /// user never wrote.
    pub folder: String,
    pub flags: DeployFlags,
}

impl DeployArgs {
    pub fn parse(args: &[String]) -> Result<DeployArgs, String> {
        let mut folder: Option<&str> = None;
        let mut flags = DeployFlags::default();
        for arg in args {
            match arg.as_str() {
                a if a.starts_with("--prebuilt=") => {
                    flags.prebuilt = Some(a["--prebuilt=".len()..].to_string())
                }
                a if a.starts_with("--site-id=") => {
                    flags.site_id = Some(a["--site-id=".len()..].to_string())
                }
                "--allow-plugins" => flags.allow_plugins = true,
                "--overwrite-newer" => flags.overwrite_newer = true,
                a if a.starts_with('-') => {
                    return Err(format!("error: unknown option '{}'\n{}", a, usage()))
                }
                a => {
                    if folder.is_some() {
                        return Err("error: deploy takes one folder".to_string());
                    }
                    folder = Some(a);
                }
            }
        }
        match folder {
            Some(folder) => Ok(DeployArgs { folder: folder.to_string(), flags }),
            None => Err(usage()),
        }
    }
}

/// `args` is everything after `moss deploy`.
pub fn run(args: &[String]) -> i32 {
    let parsed = match DeployArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{}", message);
            return 1;
        }
    };

    let root = VaultRoot::resolve(&parsed.folder);
    let path = std::path::Path::new(root.as_str());
    if let Err(code) = check_folder(path, &parsed.folder) {
        return code;
    }

    let route = DeployRoute::resolve(path, parsed.flags.prebuilt.as_deref());
    report(
        publish(path, &route, &crate::cli::host::cli_host_ports, &parsed.flags),
        &parsed.folder,
    )
}

/// The two refusals that precede any route decision, shared so the app binary
/// cannot answer them differently.
///
/// `display` is the folder as the user typed it, because a resolved absolute
/// path in an error about a folder that does not exist names something the
/// user never wrote.
pub fn check_folder(path: &std::path::Path, display: &str) -> Result<(), i32> {
    if !path.is_dir() {
        eprintln!("error: folder does not exist: {}", display);
        return Err(1);
    }
    // Refuse a folder that belongs to an existing vault, contains moss sites
    // below it, or is drive/home-shaped.
    if let Err(msg) = crate::cli::site_guard::guard_cli_open(&path.to_string_lossy(), "deploy") {
        eprintln!("error: {}", msg);
        return Err(1);
    }
    Ok(())
}

/// Run a resolved route to completion on its own runtime.
///
/// Both binaries call this, which is the point: since C4g the app's
/// `moss deploy` is not a second implementation of publish-from-a-terminal but
/// the same one, reached through `startup::headless::intercept`. `host_ports`
/// is the only thing that differs — the app supplies the real plugin runtime
/// and its config writers, the open binary a headless store.
///
/// The caller keeps the [`DeployReport`] rather than this printing it, because
/// the app has one thing to do on success that cannot cross: stamping the
/// folder for Finder is objc2/AppKit, behind the platform seam.
pub fn publish(
    folder: &std::path::Path,
    route: &DeployRoute,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    flags: &DeployFlags,
) -> Result<DeployReport, String> {
    // After the argument and folder checks, and both unconditional.
    //
    // After, because every path above returns a usage error, and
    // `deploy_argv_handling_is_identical` compares the two binaries' stderr
    // byte for byte — a global side effect on a usage error is a divergence
    // that gate would catch as a deploy problem.
    //
    // Unconditional, because the earlier version keyed the admission call off
    // `--prebuilt`: the prebuilt route loads no plugins, so that was a
    // distinction with no effect. The logger has to precede every line a
    // publish emits — without it the `=== DEPLOY START/END … status=… ===`
    // markers a failed publish is grepped by go nowhere, which is how this
    // route shipped its first version.
    crate::build::cli_output::install_headless_logger();
    // The admission verdict, before any plugin can load. `moss
    // deploy --allow-plugins` is the same consent `moss build --allow-plugins`
    // is: without it a plugin the app never allowed is refused, and its
    // content is missing from what gets published.
    crate::plugins::install::registry_client::enforce::install_headless(flags.allow_plugins);

    // Multi-thread since C4f: the hosted route runs the build pipeline here,
    // and the pipeline's content and asset halves are concurrent. The prebuilt
    // route was happy on a current-thread runtime and still is.
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => return Err(format!("failed to start runtime: {}", e)),
    };

    let sink = printing();
    runtime.block_on(async {
        match route {
            DeployRoute::Prebuilt { dir } => {
                crate::deploy::prebuilt::run_prebuilt_deploy(
                    folder,
                    dir,
                    flags.site_id.as_deref(),
                    flags.overwrite_newer,
                    &sink,
                )
                    .await
                    .map(DeployReport::from)
            }
            DeployRoute::Hosted => {
                // `Blocking`: a publish uploads what the build produced, so a
                // plugin whose content arrives after the upload has not been
                // published. `moss build`'s default is non-blocking because a
                // watcher picks the content up on the next rebuild; there is
                // no next rebuild here.
                crate::deploy::push::run_hosted_deploy(
                    folder,
                    host_ports,
                    crate::build::PluginMode::Blocking,
                    flags.site_id.as_deref(),
                    flags.overwrite_newer,
                    &sink,
                )
                .await
                .map(DeployReport::from)
            }
            // `Blocking` for the reason the hosted route gives, and more
            // sharply: the deploy hook reads the generation directly, so a
            // plugin whose content lands after it has read is not merely
            // unpublished, it is unpublished with no next rebuild to notice.
            DeployRoute::Plugin { .. } => {
                crate::deploy::plugin_push::run_plugin_deploy(
                    folder,
                    host_ports,
                    crate::build::PluginMode::Blocking,
                    &sink,
                )
                .await
                .and_then(|published| {
                    crate::deploy::plugin_push::report_for(&published)
                })
            }
        }
    })
}

/// Turn a publish outcome into what the terminal sees and the exit code.
///
/// The URL is the only thing on stdout, so `$(moss deploy .)` is the site
/// address and nothing else; progress and errors are stderr.
pub fn report(result: Result<DeployReport, String>, folder_display: &str) -> i32 {
    match result {
        Ok(DeployReport::Push(PushResult::Success { url, files_uploaded, files_removed })) => {
            println!("{}", url);
            eprintln!("published — {} uploaded, {} removed", files_uploaded, files_removed);
            0
        }
        // The plugin's own sentence, unchanged, and nothing beside it. moss
        // knows neither what the plugin uploaded nor what it removed at the
        // far end, and the plugin has already said the one thing it measured
        // — for OnionPress, whether the onion answered. A plugin that said
        // nothing gets nothing invented for it: stdout already carries the
        // address, which is this command's whole success contract.
        Ok(DeployReport::Plugin { url, message }) => {
            println!("{}", url);
            if let Some(message) = message {
                eprintln!("{}", message);
            }
            0
        }
        Ok(DeployReport::Push(PushResult::NeedsSetup)) => {
            // Since C4g this says exactly one thing, because registration is no
            // longer the app's: the folder has no environment set, and moss
            // does not mint a site — an irreversible thing to own — for a
            // folder that never named where it should live.
            eprintln!(
                "error: this folder has no site yet. Run `moss env <staging|production|local> {}` \
                 first, then deploy again.",
                folder_display
            );
            1
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

/// From the shared command table, the same line the app binary prints for a
/// deploy with no folder. `--help` never reaches here: both binaries answer it
/// from that table before dispatch.
fn usage() -> String {
    super::commands::help_for("deploy").expect("deploy is in the command table")
}

#[cfg(test)]
mod parse_tests {
    use super::DeployArgs;

    #[test]
    fn overwrite_newer_is_accepted_and_off_by_default() {
        let parse = |args: &[&str]| {
            DeployArgs::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>()).unwrap()
        };
        assert!(parse(&["site", "--overwrite-newer"]).flags.overwrite_newer);
        assert!(!parse(&["site"]).flags.overwrite_newer);
    }
}

#[cfg(test)]
mod printing_sink_tests {
    use super::PrintingSink;
    use crate::deploy::progress::{DeployProgress, DeployStage};

    fn lines(events: &[(DeployStage, &str)]) -> Vec<String> {
        let sink = PrintingSink::default();
        let mut out = Vec::new();
        for (stage, message) in events {
            let event = DeployProgress {
                stage: stage.clone(),
                current: 0,
                total: 0,
                message: (*message).to_string(),
                bytes_uploaded: None,
                bytes_total: None,
                removing: None,
                current_file: None,
            };
            if let Some(line) = sink.line(&event) {
                out.push(line.to_string());
            }
        }
        out
    }

    /// The prebuilt route emits one Uploading event per file, each naming a
    /// different file, and the hosted ticker emits one several times a second.
    /// Both collapse to a single line; nothing around them is lost.
    #[test]
    fn the_upload_stage_announces_itself_once_and_no_other_line_is_dropped() {
        assert_eq!(
            lines(&[
                (DeployStage::Preparing, "Registering site..."),
                (DeployStage::Preparing, "Building the site..."),
                (DeployStage::Syncing, "Comparing with server..."),
                (DeployStage::Uploading, "Uploading index.html (1/3)"),
                (DeployStage::Uploading, "Uploading about.html (2/3)"),
                (DeployStage::Uploading, "Uploading (3/3)"),
                (DeployStage::Committing, "Making changes live..."),
                (DeployStage::Complete, "Published"),
            ]),
            vec![
                "Registering site...",
                "Building the site...",
                "Comparing with server...",
                "Uploading index.html (1/3)",
                "Making changes live...",
                "Published",
            ]
        );
    }

    /// A publish with nothing to upload still reports every stage it does run.
    #[test]
    fn a_publish_with_no_uploads_prints_its_other_stages() {
        assert_eq!(
            lines(&[
                (DeployStage::Syncing, "Comparing with server..."),
                (DeployStage::Committing, "Making changes live..."),
            ]),
            vec!["Comparing with server...", "Making changes live..."]
        );
    }
}
