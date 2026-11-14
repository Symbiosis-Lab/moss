//! `moss deploy` — publish a folder from a terminal, in either binary.
//!
//! [`run`] is the open binary's whole command; the app enters at [`publish`]
//! from `startup::headless::intercept`, having already parsed the same
//! arguments. What each supplies is a `host_ports` factory and, in the app's
//! case, the Finder stamp that cannot cross.
//!
//! Four routes, and the folder decides which:
//! [`crate::deploy::route::DeployRoute`] reads the answer once and both
//! binaries act on it. A **prebuilt** folder — one whose site was produced by
//! another tool (Quire, Hugo, Jekyll, Astro), named by `--prebuilt=<dir>` or by
//! `[deployment].prebuilt_output` — is uploaded as-is. A folder whose
//! `[hooks] deploy` names a **plugin** is built and then handed to that plugin.
//! A folder whose `[hooks] deploy` names a host built into moss takes the
//! **direct** route, refused before the build while this version has no
//! destination for it. Everything else takes the **moss-hosted** route, where moss builds the site
//! and publishes what it sealed, registering it first if this is the folder's
//! first publish.
//!
//! No route needs a window, which is why the app's `intercept` never returns.
//!
//! Plugins in the deploy build need consent, and `--allow-plugins` is the
//! consent here that it is for `moss build`: without it a plugin the app has
//! allowed runs and one nobody allowed is refused, so its content is missing
//! from what gets published. `moss build --allow-plugins <folder>` first is the
//! way to see the decision before it reaches a live site.
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
    /// `--email <address>`: when the publish is refused because this folder's
    /// key is not verified for an account, send a sign-in link to this
    /// address, wait for it to be opened, and try again. Never sent without
    /// the flag.
    pub email: Option<String>,
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
    /// `--accept-removals`: publish even though addresses the site has served
    /// would go offline for a reason other than the author removing them. It
    /// accepts exactly the set this build found, nothing standing.
    pub accept_removals: bool,
    /// Accept a rollback identified by source timestamps or published hashes.
    pub accept_stale_content: bool,
    /// `--dry-run`: build as a deploy would, say what it would do, send
    /// nothing and write no publish state. Performance telemetry is still recorded.
    pub dry_run: bool,
    /// `--json`, with `--dry-run`: print the dry run as JSON, every list in
    /// full, for a script or an agent.
    pub json: bool,
    /// `--retry-copies=<channel>`: send again the copies on that channel whose
    /// last send stopped before the network answered.
    pub retry_copies: Vec<String>,
    /// `--copies-of=<file>`: send only this page's copies this run; with
    /// `--retry-copies`, a copy handed to the author is handed over again.
    pub copies_of: Option<String>,
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
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                a if a.starts_with("--email=") => flags.email = Some(a["--email=".len()..].to_string()),
                "--email" => match args.next() {
                    Some(address) => flags.email = Some(address.clone()),
                    None => return Err(format!("error: --email needs an address\n{}", usage())),
                },
                a if a.starts_with("--prebuilt=") => {
                    flags.prebuilt = Some(a["--prebuilt=".len()..].to_string())
                }
                a if a.starts_with("--site-id=") => {
                    flags.site_id = Some(a["--site-id=".len()..].to_string())
                }
                "--allow-plugins" => flags.allow_plugins = true,
                "--overwrite-newer" => flags.overwrite_newer = true,
                "--accept-removals" => flags.accept_removals = true,
                "--accept-stale-content" => flags.accept_stale_content = true,
                "--dry-run" => flags.dry_run = true,
                "--json" => flags.json = true,
                a if a.starts_with("--retry-copies=") => {
                    flags.retry_copies.extend(a["--retry-copies=".len()..].split(',').map(str::trim).filter(|c| !c.is_empty()).map(str::to_string))
                }
                a if a.starts_with("--copies-of=") => flags.copies_of = Some(a["--copies-of=".len()..].to_string()),
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
        if let Some(address) = &flags.email {
            crate::identity::verify_email::validate_email(address)?;
        }
        if flags.json && !flags.dry_run {
            return Err("error: --json goes with --dry-run; a deploy itself prints the published address".to_string());
        }
        if flags.dry_run && flags.accept_stale_content {
            return Err("error: --dry-run cannot accept stale content; drop --accept-stale-content to inspect the refusal".into());
        }
        if flags.dry_run && flags.accept_removals {
            return Err(
                "error: --dry-run records no publish state, so it cannot accept removals. Drop --accept-removals to see \
                 what would be refused, or drop --dry-run to publish."
                    .to_string(),
            );
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
        parsed.flags.json,
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
    runtime.block_on(crate::deploy::profile::record(folder, crate::deploy::profile::target(route), flags.dry_run, async {
    let report = dispatch(folder, route, host_ports, crate::build::PluginMode::Blocking, flags, &sink).await;
    // After the publish has returned: a copy that fails never fails it.
    if let Some(url) = report.as_ref().ok().filter(|r| r.landed()).and_then(|r| r.site_url()) {
        let print = |s: &crate::syndication::send::Sent| eprint!("{}", crate::syndication::send::render(std::slice::from_ref(s)));
        let _phase = crate::build::phase::PhaseTrace::start("after_publish");
        crate::syndication::send::after_publish(folder, url, host_ports, &flags.retry_copies, flags.copies_of.as_deref(), &print).await;
        if let Some(page) = flags.copies_of.as_deref().filter(|p| !crate::syndication::send::is_a_page(folder, p)) {
            eprintln!("--copies-of={page} names no page of this site; no copies sent");
        }
    }
    report
    }))
}

/// What `moss deploy` does once its process-wide setup is done: describe the
/// publish for `--dry-run`, otherwise run the route's own send. Apart from
/// [`publish`] so a test can reach it without installing the headless logger
/// and plugin admission, which are global to the process.
async fn dispatch(
    folder: &std::path::Path,
    route: &DeployRoute,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
    flags: &DeployFlags,
    sink: &Arc<dyn DeploySink>,
) -> Result<DeployReport, String> {
    crate::deploy::profile::record(folder, crate::deploy::profile::target(route), flags.dry_run, async {
    if let DeployRoute::Direct { id } = route {
        return Err(unavailable(id));
    }
    // Before the route's own send: a dry run is the same build with
    // everything after the gate left out, so it never reaches the match.
    if flags.dry_run {
        return match route {
            DeployRoute::Prebuilt { dir } => crate::deploy::dry_run::run_prebuilt_dry_run(
                folder,
                dir,
                flags.site_id.as_deref(),
                flags.accept_removals,
                flags.overwrite_newer,
            )
            .await
            .map(|dry| DeployReport::DryRun(Box::new(dry))),
            _ => crate::deploy::dry_run::run_dry_run(folder, route, flags.site_id.as_deref(), host_ports, plugins, flags.overwrite_newer)
                .await
                .map(|dry| DeployReport::DryRun(Box::new(dry))),
        };
    }
    crate::identity::verify_email::retrying_after_verification(
        folder,
        flags.email.as_deref(),
        Some(&crate::deploy::first_publish_site_id(folder, flags.site_id.as_deref())),
        crate::identity::verify_email::POLL_INTERVAL,
        crate::identity::verify_email::WAIT_LIMIT,
        || send(folder, route, host_ports, plugins, flags, sink),
    )
    .await
    .map_err(|e| e.cli_text(folder))
    }).await
}

/// The route's own send, once.
async fn send(
    folder: &std::path::Path,
    route: &DeployRoute,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
    flags: &DeployFlags,
    sink: &Arc<dyn DeploySink>,
) -> Result<DeployReport, crate::deploy::PublishError> {
    crate::deploy::profile::begin_attempt();
    match route {
        DeployRoute::Prebuilt { dir } => {
            crate::deploy::prebuilt::run_prebuilt_deploy(
                folder,
                dir,
                flags.site_id.as_deref(),
                flags.overwrite_newer,
                flags.accept_removals,
                sink,
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
                plugins,
                flags.site_id.as_deref(),
                crate::deploy::push::PublishOverrides {
                    overwrite_newer: flags.overwrite_newer,
                    accept_removals: flags.accept_removals,
                    accept_stale_content: flags.accept_stale_content,
                },
                sink,
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
                plugins,
                crate::deploy::push::PublishOverrides {
                    accept_removals: flags.accept_removals,
                    accept_stale_content: flags.accept_stale_content,
                    ..Default::default()
                },
                sink,
            )
            .await
            .and_then(|published| {
                crate::deploy::plugin_push::report_for(&published)
            })
            .map_err(Into::into)
        }
        DeployRoute::Direct { id } => Err(unavailable(id).into()),
    }
}

/// Why a built-in target this version has no destination for is refused.
/// Known at once, so asked before the build and before a dry run.
fn unavailable(id: &str) -> String {
    format!("this version of moss cannot publish to [hooks] deploy = \"{id}\" yet")
}

/// Turn a publish outcome into what the terminal sees and the exit code.
///
/// The URL is the only thing on stdout, so `$(moss deploy .)` is the site
/// address and nothing else; progress and errors are stderr.
/// `json` is `--json`: the dry run, or any failure, printed as JSON instead.
pub fn report(result: Result<DeployReport, String>, folder_display: &str, json: bool) -> i32 {
    if json {
        let (out, code) = json_report(&result);
        println!("{out}");
        return code;
    }
    match result {
        Ok(DeployReport::Push(PushResult::Success { url, files_uploaded, files_removed })) => {
            println!("{}", url);
            eprintln!("{}", published_line(folder_display, &url, files_uploaded, files_removed));
            eprint!("{}", removed_by_author(folder_display));
            0
        }
        // The plugin's own sentence, unchanged, and nothing beside it. moss
        // knows neither what the plugin uploaded nor what it removed at the
        // far end, and the plugin has already said the one thing it measured
        // — for OnionPress, whether the onion answered. A plugin that said
        // nothing gets nothing invented for it: stdout already carries the
        // address, which is this command's whole success contract.
        Ok(DeployReport::DryRun(dry)) => {
            // The report is the command's product, so it is stdout.
            print!("{}", dry.render());
            if dry.refusal.is_some() { 1 } else { 0 }
        }
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
            eprintln!("error: {}", crate::deploy::dry_run::no_site_message(folder_display));
            1
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

/// The line a landed publish prints on stderr: which folder went where.
fn published_line(folder: &str, url: &str, uploaded: u32, removed: u32) -> String {
    let name = crate::vault::paths::VaultRoot::resolve(folder).name().to_string();
    format!("Published {name} to {url} — {uploaded} uploaded, {removed} removed")
}

#[test]
fn a_landed_publish_names_the_folder_and_where_it_went() {
    assert_eq!(
        published_line("/Users/me/Sites/My Blog/", "https://my-blog.mosspub.com", 3, 1),
        "Published My Blog to https://my-blog.mosspub.com — 3 uploaded, 1 removed"
    );
}

/// The addresses the author deleted, named, for after a publish: what the
/// bare removed count could not say. Empty when there are none. Read from the
/// build this process just ran, so a folder this process did not build says
/// nothing.
fn removed_by_author(folder: &str) -> String {
    use crate::build::manifest::change_set::RemovalReason;
    let gone: Vec<_> = crate::system::build_records::records()
        .removed_addresses(folder)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| !r.reason.refuses())
        .collect();
    if gone.is_empty() {
        return String::new();
    }
    format!(
        "addresses you removed, no longer served:\n{}",
        crate::deploy::removal_gate::capped_lines(&gone, crate::deploy::removal_gate::cause_line)
    )
}

/// From the shared command table, the same line the app binary prints for a
/// deploy with no folder. `--help` never reaches here: both binaries answer it
/// from that table before dispatch.
/// `--dry-run --json`'s stdout and exit code: the dry run, or an `{"error"}`
/// object for any failure, so a script reads JSON on every path.
fn json_report(result: &Result<DeployReport, String>) -> (String, i32) {
    match result {
        Ok(DeployReport::DryRun(dry)) => {
            (serde_json::to_string_pretty(&dry.to_json()).expect("a dry run serializes"), if dry.refusal.is_some() { 1 } else { 0 })
        }
        Ok(_) => (super::list::json_error("--json describes a dry run, and this was a deploy"), 1),
        Err(e) => (super::list::json_error(e), 1),
    }
}

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

#[cfg(test)]
mod removed_by_author_tests {
    use super::removed_by_author;
    use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};

    fn gone(path: &str, reason: RemovalReason, source: Option<&str>) -> RemovedAddress {
        RemovedAddress { path: path.into(), reason, moved_to: None, source: source.map(Into::into), withdrawn_by: None }
    }

    /// After a publish the author reads which addresses went away because
    /// they deleted the source; the ones they accepted losing are their own
    /// decision and are not repeated.
    #[test]
    fn a_publish_names_the_addresses_the_author_deleted() {
        let records = crate::system::build_records::records();
        records.record_removed_addresses(
            "/report-removed",
            vec![
                gone("gone/index.html", RemovalReason::AuthorRemoved, Some("gone.md")),
                gone("legacy-feed.xml", RemovalReason::Unexplained, None),
            ],
        );
        assert_eq!(
            removed_by_author("/report-removed"),
            "addresses you removed, no longer served:\n  /gone/ is gone because you deleted its source (gone.md).\n"
        );
        records.record_removed_addresses("/report-removed", Vec::new());
        assert_eq!(removed_by_author("/report-removed"), "");
    }
}

#[cfg(test)]
mod removal_flag_tests {
    use super::DeployArgs;

    fn parse(args: &[&str]) -> Result<DeployArgs, String> {
        DeployArgs::parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn accept_removals_and_dry_run_are_accepted_and_off_by_default() {
        let both = parse(&["site", "--accept-removals"]).unwrap().flags;
        assert!(both.accept_removals && !both.dry_run);
        let dry = parse(&["site", "--dry-run"]).unwrap().flags;
        assert!(dry.dry_run && !dry.accept_removals);
        let plain = parse(&["site"]).unwrap().flags;
        assert!(!plain.dry_run && !plain.accept_removals);
    }

    #[test]
    fn stale_content_acceptance_is_explicit_and_not_a_dry_run() {
        assert!(!parse(&["site"]).unwrap().flags.accept_stale_content);
        assert!(parse(&["site", "--accept-stale-content"]).unwrap().flags.accept_stale_content);
        assert!(parse(&["site", "--dry-run", "--accept-stale-content"]).is_err());
    }

    #[test]
    fn email_is_accepted_both_ways_and_must_look_like_an_address() {
        assert_eq!(parse(&["site", "--email", "a@b.co"]).unwrap().flags.email.as_deref(), Some("a@b.co"));
        assert_eq!(parse(&["site", "--email=a@b.co"]).unwrap().flags.email.as_deref(), Some("a@b.co"));
        assert_eq!(parse(&["site"]).unwrap().flags.email, None);
        assert!(parse(&["site", "--email"]).unwrap_err().contains("needs an address"));
        assert!(parse(&["site", "--email", "nope"]).unwrap_err().contains("not an email address"));
    }

    #[test]
    fn retry_copies_takes_channels_by_comma_or_by_repeating_the_flag() {
        let flags = parse(&["site", "--retry-copies=bluesky, mastodon", "--retry-copies=x,"]).unwrap().flags;
        assert_eq!(flags.retry_copies, ["bluesky", "mastodon", "x"]);
        assert!(parse(&["site"]).unwrap().flags.retry_copies.is_empty());
        assert_eq!(parse(&["site", "--copies-of=notes/a.md"]).unwrap().flags.copies_of.as_deref(), Some("notes/a.md"));
    }

    #[test]
    fn a_failed_json_dry_run_still_prints_json() {
        let (out, code) = super::json_report(&Err("the build failed".to_string()));
        assert_eq!(code, 1);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap()["error"], "the build failed");
    }

    #[test]
    fn json_goes_with_a_dry_run_only() {
        assert!(parse(&["site", "--dry-run", "--json"]).unwrap().flags.json);
        let err = parse(&["site", "--json"]).unwrap_err();
        assert!(err.starts_with("error: --json goes with --dry-run"), "{err}");
    }

    /// A dry run records nothing, so it cannot also record an acceptance.
    #[test]
    fn a_dry_run_cannot_accept_removals() {
        let err = parse(&["site", "--dry-run", "--accept-removals"]).unwrap_err();
        assert!(err.starts_with("error: --dry-run records no publish state"), "{err}");
    }
}

/// `moss deploy --dry-run`, through the same `publish` and `report` the
/// command runs. The fixture's last publish listed `feed.xml`, which this
/// build does not produce, so a real deploy of it would be refused.
#[cfg(test)]
mod dry_run_tests {
    use super::*;
    use crate::build::manifest::change_set::PublishedSnapshot;
    use crate::moss_paths::MossPaths;

    use crate::seta::client::tests::StandIn;

    /// The one read a dry run sends a folder with a site and a key.
    const GENERATION: &str = "GET /api/sites/dry-test/generation";

    struct Fixture {
        dir: tempfile::TempDir,
        seta: StandIn,
        _env: std::sync::MutexGuard<'static, ()>,
        prev_url: Option<String>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            match self.prev_url.take() {
                Some(u) => std::env::set_var("MOSS_SETA_URL", u),
                None => std::env::remove_var("MOSS_SETA_URL"),
            }
        }
    }

    impl Fixture {
        /// A one-page folder with a site and a signing key; `lost_feed` adds a
        /// record of a publish that served `legacy-feed.xml`. The hosting
        /// server says the key owns the site, which has never published.
        async fn new(lost_feed: bool) -> Self {
            Self::answered_by(lost_feed, StandIn::json(200, r#"{"generation_id":null,"deployed_at":null}"#).await)
        }

        fn answered_by(lost_feed: bool, seta: StandIn) -> Self {
            let env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/hosted");
            std::fs::create_dir_all(&base).unwrap();
            let dir = tempfile::Builder::new().prefix("dry").tempdir_in(&base).unwrap();
            std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
            std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
            std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"dry-test\"\n").unwrap();
            crate::identity::Identity::generate().unwrap().save(dir.path()).unwrap();
            if lost_feed {
                crate::build::manifest::published_record::save(
                    &MossPaths::new(dir.path()),
                    &PublishedSnapshot {
                        generation_id: "ours".into(),
                        target: "moss:dry-test".into(),
                        published_at: "2026-09-20T09:12:30+00:00".into(),
                        files: [("legacy-feed.xml".to_string(), "100644:abc".to_string())].into(),
                        asset_source_to_output: Some(Default::default()),
                        ..Default::default()
                    },
                )
                .unwrap();
            }
            let prev_url = std::env::var("MOSS_SETA_URL").ok();
            std::env::set_var("MOSS_SETA_URL", seta.url());
            Self { dir, seta, _env: env, prev_url }
        }

        /// What the hosting server received, as "METHOD path".
        fn requests(&self) -> Vec<String> {
            self.seta.requests().iter().map(|r| format!("{} {}", r.method, r.path_and_query)).collect()
        }

        async fn run(&self, dry_run: bool) -> Result<DeployReport, String> {
            self.run_with(DeployFlags { dry_run, ..DeployFlags::default() }).await
        }

        async fn run_with(&self, flags: DeployFlags) -> Result<DeployReport, String> {
            self.run_on(&DeployRoute::Hosted, flags).await
        }

        async fn run_on(&self, route: &DeployRoute, flags: DeployFlags) -> Result<DeployReport, String> {
            dispatch(self.dir.path(), route, &crate::cli::host::cli_host_ports, crate::build::PluginMode::Skip, &flags, &printing()).await
        }

        fn record_bytes(&self) -> Vec<u8> {
            let path = crate::build::manifest::published_record::path_for(&MossPaths::new(self.dir.path()), "moss:dry-test");
            std::fs::read(path).unwrap_or_default()
        }
    }

    /// `--json` gives the whole plan to a script: every moved address, and the
    /// `[redirects]` lines that keep them, which the text report cuts at 20.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_json_dry_run_lists_every_move_and_the_lines_that_keep_them() {
        let fx = Fixture::new(false).await;
        // A page that moved, beside 25 files that moved with their folder.
        std::fs::write(fx.dir.path().join("moved.md"), "---\ntitle: Moved\n---\n\nNew home.\n").unwrap();
        let mut files = std::collections::HashMap::from([("old/index.html".to_string(), "100644:abc".to_string())]);
        let mut assets = std::collections::HashMap::new();
        std::fs::create_dir_all(fx.dir.path().join("review/new")).unwrap();
        for i in 0..25 {
            let bytes = format!("%PDF-1.4 {i}");
            std::fs::write(fx.dir.path().join(format!("review/new/{i:02}.pdf")), &bytes).unwrap();
            let old = format!("review/old/{i:02}.pdf");
            files.insert(old.clone(), crate::types::content::file_entry(&crate::build::assets::paths::compute_binary_hash(bytes.as_bytes())));
            assets.insert(old.clone(), old);
        }
        crate::build::manifest::published_record::save(
            &MossPaths::new(fx.dir.path()),
            &PublishedSnapshot {
                generation_id: "ours".into(),
                target: "moss:dry-test".into(),
                published_at: "2026-09-20T09:12:30+00:00".into(),
                files,
                source_to_output: [("moved.md".to_string(), "old/index.html".to_string())].into(),
                asset_source_to_output: Some(assets),
                ..Default::default()
            },
        )
        .unwrap();
        let flags = DeployFlags { dry_run: true, json: true, ..DeployFlags::default() };
        let report = fx.run_with(flags).await.expect("a dry run reports");

        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };
        assert!(dry.refusal.is_some());
        let (out, code) = super::json_report(&Ok(report));
        assert_eq!(code, 1, "a refused dry run exits 1 in JSON too");
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        let offline = json["addresses_going_offline"].as_array().unwrap();
        assert_eq!(offline.len(), 26, "every address, not the first 20");
        assert_eq!(offline[0]["output_path"], "old/index.html");
        assert_eq!(offline[0]["address"], "/old/");
        assert_eq!(offline[0]["moved_to"], "/moved/");
        assert!(json["change_set"].get("removed").is_none(), "one list of offline addresses, named");
        assert!(json["transfer"]["removed_output_paths"].as_array().unwrap().iter().any(|p| p == "review/old/00.pdf"));
        assert_eq!(json["refusal"]["kind"], "removals");
        let pending = json["pending_removals"].as_array().unwrap();
        assert_eq!(pending.len(), 26);
        assert_eq!(pending[1]["output_path"], "review/old/00.pdf");
        assert_eq!(pending[1]["address"], "/review/old/00.pdf");
        assert_eq!(pending[1]["cause"], serde_json::json!({ "kind": "moved", "to": "/review/new/00.pdf" }));
        assert_eq!(json["redirects_to_add"], serde_json::json!([r#""/old/" = "/moved/""#, r#""/review/old/" = "/review/new/""#]));
        assert_eq!(fx.requests(), [GENERATION]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_leaves_the_source_folder_byte_identical() {
        let fx = Fixture::new(false).await;
        let note = "---\ntitle: Note\n---\n\nShared text.\n";
        std::fs::write(fx.dir.path().join("note.md"), note).unwrap();
        let index_before = std::fs::read(fx.dir.path().join("index.md")).unwrap();

        fx.run(true).await.expect("a dry run reports");

        assert_eq!(std::fs::read_to_string(fx.dir.path().join("note.md")).unwrap(), note);
        assert_eq!(std::fs::read(fx.dir.path().join("index.md")).unwrap(), index_before);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_that_a_real_deploy_would_refuse_exits_non_zero_and_uploads_nothing() {
        let fx = Fixture::new(true).await;
        let before = fx.record_bytes();

        let (host, mut warnings) = crate::ops::serve::events::collect_removal_warnings();
        let flags = DeployFlags { dry_run: true, ..DeployFlags::default() };
        let report = dispatch(fx.dir.path(), &DeployRoute::Hosted, &host, crate::build::PluginMode::Skip, &flags, &printing())
            .await.expect("a dry run reports, it does not fail");
        assert!(warnings().iter().any(|w| w.contains("/legacy-feed.xml")));
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.refusal.as_ref().map(|r| r.text.as_str()).is_some_and(|r| r.contains("/legacy-feed.xml")), "{dry:?}");
        assert!(
            matches!(&dry.transfer, crate::deploy::dry_run::Transfer::SinceLastPublish { removed, .. } if removed == &["legacy-feed.xml"]),
            "{:?}",
            dry.transfer
        );
        assert_eq!(report_code(report), 1);
        assert_eq!(fx.requests(), [GENERATION], "a dry run asks one thing and sends nothing");
        assert_eq!(fx.record_bytes(), before, "a dry run must not write a published record");
        let key = fx.dir.path().to_string_lossy().to_string();
        assert!(
            crate::system::build_records::records().accepted_removals(&key).is_empty(),
            "a dry run must not record an acceptance"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_derivative_the_build_stops_producing_is_kept_as_a_redirect_and_the_check_passes() {
        let fx = Fixture::new(false).await;
        std::fs::write(fx.dir.path().join("photo-2.jpg"), b"JPEG").unwrap();
        crate::build::manifest::published_record::save(
            &MossPaths::new(fx.dir.path()),
            &PublishedSnapshot {
                generation_id: "ours".into(),
                target: "moss:dry-test".into(),
                published_at: "2026-09-20T09:12:30+00:00".into(),
                files: [
                    ("photo-2.jpg".to_string(), "100644:abc".to_string()),
                    ("photo-2.webp".to_string(), "100644:def".to_string()),
                ]
                .into(),
                asset_source_to_output: Some([("photo-2.jpg".to_string(), "photo-2.jpg".to_string())].into()),
                ..Default::default()
            },
        )
        .unwrap();

        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.refusal.is_none(), "{dry:?}");
        assert_eq!(dry.kept_as_redirect, [crate::deploy::dry_run::Kept { from: "/photo-2.webp".into(), to: "/photo-2.jpg".into() }]);
        assert!(dry.render().contains("/photo-2.webp -> /photo-2.jpg"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_a_real_deploy_would_not_refuse_exits_zero_and_uploads_nothing() {
        let fx = Fixture::new(false).await;

        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.refusal.is_none(), "{dry:?}");
        assert!(matches!(dry.transfer, crate::deploy::dry_run::Transfer::NoRecord), "no record, so there is nothing to count against: {dry:?}");
        assert_eq!(report_code(report), 0);
        assert_eq!(fx.requests(), [GENERATION], "a dry run asks one thing and sends nothing");
        assert!(fx.record_bytes().is_empty(), "nothing published, so no record");
    }

    async fn dry_run_with(fx: &Fixture, flags: DeployFlags) -> crate::deploy::dry_run::DryRun {
        let report = fx.run_with(flags).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = report else { panic!("expected a dry run, got {report:?}") };
        *dry
    }

    async fn dry_run_against(fx: &Fixture) -> crate::deploy::dry_run::DryRun {
        dry_run_with(fx, DeployFlags { dry_run: true, ..DeployFlags::default() }).await
    }

    /// A dry run of the folder's `_site`, holding one page, as a prebuilt publish.
    async fn prebuilt_dry_run(fx: &Fixture) -> crate::deploy::dry_run::DryRun {
        let site = fx.dir.path().join("_site");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.html"), "<p>hi</p>").unwrap();
        let route = DeployRoute::Prebuilt { dir: site };
        let report = fx.run_on(&route, DeployFlags { dry_run: true, ..DeployFlags::default() }).await.expect("a prebuilt dry run reports");
        let DeployReport::DryRun(dry) = report else { panic!("expected a dry run, got {report:?}") };
        *dry
    }

    fn answered(access: crate::deploy::dry_run::SiteAccess) -> crate::deploy::dry_run::Owner {
        crate::deploy::dry_run::Owner::Answered { site: "dry-test".into(), access }
    }

    fn unchecked_reason(dry: &crate::deploy::dry_run::DryRun) -> &str {
        let crate::deploy::dry_run::Owner::Unchecked { reason } = &dry.owner else { panic!("{:?}", dry.owner) };
        reason
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_asks_the_server_whether_this_key_may_publish_and_sends_nothing_else() {
        let fx = Fixture::answered_by(false, StandIn::json(200, r#"{"generation_id":"live-gen-0001","deployed_at":1}"#).await);
        let dry = dry_run_against(&fx).await;
        assert_eq!(dry.owner, answered(crate::deploy::dry_run::SiteAccess::Authorized));
        assert!(dry.render().contains("may publish the site \"dry-test\""), "{}", dry.render());
        assert_eq!(dry.to_json()["owner"], serde_json::json!({ "state": "answered", "site": "dry-test", "access": "authorized" }));
        assert!(dry.refusal.is_none(), "{dry:?}");
        assert_eq!(fx.requests(), [GENERATION], "one read, no upload");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_key_that_does_not_own_the_site_is_refused_with_the_email_fix() {
        let fx = Fixture::answered_by(false, StandIn::json(403, r#"{"error":"Not authorized for this site"}"#).await);
        let dry = dry_run_against(&fx).await;
        assert_eq!(dry.owner, answered(crate::deploy::dry_run::SiteAccess::NotOwner));
        let refusal = dry.refusal.as_ref().expect("a real deploy would be refused");
        assert_eq!(refusal.kind, crate::deploy::RefusalKind::NotOwner);
        assert!(refusal.text.contains("--email"), "{}", refusal.text);
    }

    /// The owner's key not signed in to this site is refused with the sentence
    /// `--email` lifts, naming the address the server gave.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_folder_not_signed_in_to_its_site_is_refused_with_the_sign_in_sentence() {
        let body = r#"{"error":"Not authorized for this site","code":"folder_not_signed_in","site":"dry-test.example.org","detail":"This folder isn't signed in to dry-test.example.org."}"#;
        let fx = Fixture::answered_by(false, StandIn::json(403, body).await);
        let dry = dry_run_against(&fx).await;
        assert_eq!(dry.owner, answered(crate::deploy::dry_run::SiteAccess::NotSignedIn));
        let refusal = dry.refusal.as_ref().expect("a real deploy would be refused");
        assert_eq!(refusal.kind, crate::deploy::RefusalKind::NotSignedIn);
        assert_eq!(refusal.text, crate::identity::verify_email::not_signed_in(fx.dir.path(), "dry-test.example.org"));
    }

    /// The prebuilt route asks the same question and refuses the same way.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_prebuilt_dry_run_refuses_a_key_that_does_not_own_the_site() {
        let fx = Fixture::answered_by(false, StandIn::json(403, r#"{"error":"Not authorized for this site"}"#).await);
        let dry = prebuilt_dry_run(&fx).await;
        assert_eq!(dry.refusal.as_ref().map(|r| r.kind), Some(crate::deploy::RefusalKind::NotOwner), "{dry:?}");
        assert_eq!(fx.requests(), [GENERATION]);
    }

    /// A 404 is a site the server does not know or a server too old to have
    /// the route, so the dry run claims neither and refuses nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_site_the_server_does_not_know_is_not_known_and_not_a_refusal() {
        let fx = Fixture::answered_by(false, StandIn::json(404, r#"{"error":"Site not found"}"#).await);
        let dry = dry_run_against(&fx).await;
        assert_eq!(dry.owner, answered(crate::deploy::dry_run::SiteAccess::NotFound));
        assert!(dry.render().contains("Publishing: not known"), "{}", dry.render());
        assert!(dry.refusal.is_none(), "{dry:?}");
    }

    /// A 401 is a clock or signing fault, not another owner: `--email` would
    /// not fix it, so it refuses nothing and says what the server said.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_rejected_signature_is_unchecked_not_a_refusal() {
        let fx = Fixture::answered_by(false, StandIn::json(401, r#"{"error":"Request timestamp expired"}"#).await);
        let dry = dry_run_against(&fx).await;
        assert!(unchecked_reason(&dry).contains("Request timestamp expired"), "{dry:?}");
        assert!(dry.render().contains("and whether another copy published since"), "{}", dry.render());
        assert!(dry.refusal.is_none(), "{dry:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_edge_challenge_is_unchecked_not_a_refusal() {
        let fx = Fixture::answered_by(false, StandIn::new(403, &[("cf-mitigated", "challenge")], |_, _| String::new()).await);
        let dry = dry_run_against(&fx).await;
        unchecked_reason(&dry);
        assert!(dry.refusal.is_none(), "{dry:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreachable_server_is_unchecked_and_the_dry_run_still_reports() {
        let fx = Fixture::new(false).await;
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        std::env::set_var("MOSS_SETA_URL", format!("http://{closed}"));
        let dry = dry_run_against(&fx).await;
        assert!(unchecked_reason(&dry).contains("could not be asked"), "{dry:?}");
        assert!(dry.refusal.is_none(), "{dry:?}");
    }

    /// Loading an identity in the format before the current one rewrites it,
    /// so a dry run does not sign with it and leaves the file as it was.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_older_identity_format_is_not_asked_with_and_not_rewritten() {
        let fx = Fixture::new(false).await;
        let public = fx.dir.path().join(".moss/identity/public.json");
        let mut raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&public).unwrap()).unwrap();
        raw["version"] = 2.into();
        std::fs::write(&public, raw.to_string()).unwrap();
        let before = std::fs::read(&public).unwrap();
        let dry = dry_run_against(&fx).await;
        assert!(unchecked_reason(&dry).contains("current format"), "{dry:?}");
        assert_eq!(std::fs::read(&public).unwrap(), before, "the dry run must not migrate it");
        assert!(fx.requests().is_empty(), "{:?}", fx.requests());
    }

    /// A deploy refuses a key file from a newer moss, so a dry run does not ask with it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_newer_identity_format_is_not_asked_with() {
        let fx = Fixture::new(false).await;
        let public = fx.dir.path().join(".moss/identity/public.json");
        let mut raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&public).unwrap()).unwrap();
        raw["version"] = 99.into();
        std::fs::write(&public, raw.to_string()).unwrap();
        let before = std::fs::read(&public).unwrap();
        let dry = dry_run_against(&fx).await;
        assert!(unchecked_reason(&dry).contains("a newer moss"), "{dry:?}");
        assert_eq!(std::fs::read(&public).unwrap(), before);
        assert!(fx.requests().is_empty(), "{:?}", fx.requests());
    }

    /// The same answer says whether another copy published after this one,
    /// which a deploy refuses unless `--overwrite-newer`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_newer_publish_from_another_copy_is_refused_unless_overwrite_newer() {
        // 2026-09-24, after this folder's record.
        let fx = Fixture::answered_by(false, StandIn::json(200, r#"{"generation_id":"theirs","deployed_at":1790258580}"#).await);
        crate::build::manifest::published_record::save(
            &MossPaths::new(fx.dir.path()),
            &PublishedSnapshot {
                generation_id: "ours".into(),
                target: "moss:dry-test".into(),
                published_at: "2026-09-20T09:12:30+00:00".into(),
                asset_source_to_output: Some(Default::default()),
                ..Default::default()
            },
        )
        .unwrap();
        let dry = dry_run_against(&fx).await;
        let refusal = dry.refusal.as_ref().expect("a real deploy would be refused");
        assert_eq!(refusal.kind, crate::deploy::RefusalKind::StaleCopy, "{}", refusal.text);
        assert!(refusal.text.contains("--overwrite-newer"), "{}", refusal.text);

        let dry = dry_run_with(&fx, DeployFlags { dry_run: true, overwrite_newer: true, ..DeployFlags::default() }).await;
        assert!(dry.refusal.is_none(), "{dry:?}");
    }

    /// A folder that never published: no `site_id`, and `moss env` run or not.
    async fn unpublished(environment: bool) -> Fixture {
        let fx = Fixture::new(false).await;
        std::fs::remove_file(fx.dir.path().join(".moss/state.toml")).unwrap();
        std::fs::remove_dir_all(fx.dir.path().join(".moss/identity")).unwrap();
        if environment {
            std::fs::write(fx.dir.path().join(".moss/config.toml"), "environment = \"staging\"\n").unwrap();
        }
        fx
    }

    /// A folder with no site yet still gets its plan from a dry run; the missing
    /// environment is what a real deploy needs first, named as its refusal.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_folder_with_no_site_and_no_environment_dry_run_builds_and_names_the_missing_environment() {
        let fx = unpublished(false).await;
        let report = fx.run(true).await.expect("a dry run reports the plan");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };
        let refusal = dry.refusal.as_ref().map(|r| r.text.as_str()).expect("a real deploy would stop here");
        assert!(refusal.contains("has no site yet") && refusal.contains("moss env"), "{refusal}");
        assert!(fx.requests().is_empty());
    }

    /// The refusal a first publish would meet is knowable from the folder: no
    /// email is recorded for its key. Saying so sends nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_first_publish_dry_run_says_verification_would_be_needed_and_sends_nothing() {
        let fx = unpublished(true).await;
        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.needs_verification, "{dry:?}");
        assert!(dry.render().contains("--email <address>"), "{}", dry.render());
        assert!(fx.requests().is_empty(), "a dry run must not contact the server");
    }

    /// A registered site whose key is gone is not signed in: a new key would be
    /// one the hosting service has never seen. A deploy refuses before it reaches
    /// the server and writes no key; a dry run names the same refusal.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_registered_site_that_lost_its_key_is_refused_as_not_signed_in() {
        let fx = Fixture::new(false).await;
        let identity = fx.dir.path().join(".moss/identity");
        std::fs::remove_dir_all(&identity).unwrap();
        crate::identity::Identity::generate().unwrap().save(fx.dir.path()).unwrap();
        let pubkey = crate::identity::Identity::read_pubkey(fx.dir.path());
        std::fs::remove_file(identity.join("secret-key")).unwrap();

        let name = crate::vault::paths::VaultRoot::resolve(fx.dir.path()).name().to_string();
        let host = crate::deploy::sign_in_host(&fx.dir.path().to_string_lossy(), Some("dry-test"));
        assert!(host.starts_with("dry-test."), "the site's address, not the service: {host}");
        let expected = format!("{name} isn't signed in to {host}. Sign it in: moss deploy {} --email <address>", fx.dir.path().display());
        let err = fx.run(false).await.expect_err("a lost key must refuse");
        assert_eq!(err, expected);
        assert_eq!(crate::identity::Identity::read_pubkey(fx.dir.path()), pubkey, "the old key stays named");
        assert!(!identity.join("secret-key").exists(), "no new key may be written");
        assert!(fx.requests().is_empty(), "nothing may reach the server");

        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };
        let refusal = dry.refusal.as_ref().expect("a real deploy would stop here");
        assert_eq!((refusal.kind, refusal.text.as_str()), (crate::deploy::RefusalKind::NotSignedIn, expected.as_str()));
        assert!(!dry.would_create_key, "{dry:?}");
    }

    /// The same for a site folder with no identity at all, as a git clone has.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_registered_site_with_no_identity_is_refused_and_given_none() {
        let fx = Fixture::new(false).await;
        let identity = fx.dir.path().join(".moss/identity");
        std::fs::remove_dir_all(&identity).unwrap();

        let err = fx.run(false).await.expect_err("no identity must refuse");
        assert!(err.contains(" isn't signed in to dry-test."), "{err}");
        assert!(!identity.exists(), "no identity may be written");
        assert!(fx.requests().is_empty(), "nothing may reach the server");
    }

    /// An old identity keeps its private key inside `public.json`; a deploy
    /// migrates it and signs with it, so it is not "no key" and a dry run must
    /// not say a new one would be created (a new key is a different owner).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_legacy_identity_with_its_key_in_public_json_is_not_reported_as_missing() {
        let fx = Fixture::new(false).await;
        std::fs::remove_file(fx.dir.path().join(".moss/identity/secret-key")).unwrap();
        std::fs::write(
            fx.dir.path().join(".moss/identity/public.json"),
            format!(r#"{{"pubkey":"{0}","privkey":"{0}"}}"#, "ab".repeat(32)),
        )
        .unwrap();
        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };
        assert!(!dry.would_create_key, "{dry:?}");
        assert!(!dry.render().contains("create a new one"), "{}", dry.render());
        assert!(
            fx.dir.path().join(".moss/identity/public.json").metadata().unwrap().len() > 0
                && !fx.dir.path().join(".moss/identity/secret-key").exists(),
            "the dry run must not migrate it"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_config_from_a_newer_moss_fails_the_dry_run_as_it_fails_a_deploy() {
        let fx = Fixture::new(false).await;
        let future = crate::config::migrations::CURRENT_VERSION + 1;
        std::fs::write(fx.dir.path().join(".moss/config.toml"), format!("schema_version = {future}\n")).unwrap();
        assert!(fx.run(true).await.is_err());
    }

    /// A first publish registers a site and creates a key; the dry run does
    /// neither, and says what it would do.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_first_publish_dry_run_registers_nothing_and_creates_no_key() {
        let fx = unpublished(true).await;
        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.would_register.is_some(), "{dry:?}");
        assert!(dry.render().contains("would register the site"), "{}", dry.render());
        assert!(!fx.dir.path().join(".moss/identity").exists(), "no key may be created");
        assert!(!fx.dir.path().join(".moss/state.toml").exists(), "no site may be registered");
        assert!(fx.requests().is_empty());
    }

    /// A prebuilt folder is described from the files on disk, not refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_of_a_prebuilt_folder_describes_it_and_uploads_nothing() {
        let fx = Fixture::new(true).await;
        let dry = prebuilt_dry_run(&fx).await;
        assert!(dry.prebuilt && dry.refusal.is_some(), "{dry:?}");
        assert_eq!(fx.requests(), [GENERATION], "one read, no upload");
    }

    /// The plugin route reads the same gate: refused without the flag, and
    /// past it with the flag (this folder names no deploy plugin, so what
    /// comes back after the gate is the plugin route's own "no plugin").
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_plugin_route_honours_accept_removals() {
        let fx = Fixture::new(true).await;
        let sink = crate::deploy::progress::silent();
        let (host, mut warnings) = crate::ops::serve::events::collect_removal_warnings();
        let run = |accept| {
            crate::deploy::plugin_push::run_plugin_deploy(
                fx.dir.path(),
                &host,
                crate::build::PluginMode::Skip,
                crate::deploy::push::PublishOverrides { accept_removals: accept, ..Default::default() },
                &sink,
            )
        };
        let refused = run(false).await.expect_err("an unexplained removal stops a plugin publish too");
        assert!(refused.contains("/legacy-feed.xml"), "{refused}");

        assert!(warnings().iter().any(|w| w.contains("/legacy-feed.xml")));

        let past = run(true).await;
        assert!(!matches!(&past, Err(e) if e.contains("legacy-feed.xml")), "accepted, so not the gate: {past:?}");
        assert!(warnings().is_empty(), "accepted removals must not ask for action");
        let removed = crate::system::build_records::records()
            .removed_addresses(&fx.dir.path().to_string_lossy()).unwrap();
        assert!(removed.iter().any(|r| r.path == "legacy-feed.xml"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_content_is_reported_by_dry_run_and_accepted_on_the_plugin_route() {
        let fx = Fixture::new(false).await;
        let dir = &fx.dir;
        let history = dir.path().join(".moss/history/publishes");
        std::fs::create_dir_all(&history).unwrap();
        std::fs::write(history.join("2026-01-01.json"), serde_json::json!({
            "version": 1, "published_at": "2026-01-01T00:00:00Z", "target": "moss:dry-test",
            "generation_id": "previous", "entries": {
                "index.md": {"mode": "100644", "hash": "previous", "source_mtime": [4000000000u64, 0]}
            }
        }).to_string()).unwrap();
        let flags = DeployFlags { dry_run: true, ..Default::default() };
        let report = dispatch(dir.path(), &DeployRoute::Hosted, &crate::cli::host::cli_host_ports,
            crate::build::PluginMode::Skip, &flags, &printing()).await.unwrap();
        let DeployReport::DryRun(dry) = report else { panic!("expected dry run") };
        assert_eq!(dry.refusal.unwrap().kind, crate::deploy::RefusalKind::StaleContent);
        let sink = crate::deploy::progress::silent();
        let run = |accept| crate::deploy::plugin_push::run_plugin_deploy(
            dir.path(), &crate::cli::host::cli_host_ports, crate::build::PluginMode::Skip,
            crate::deploy::push::PublishOverrides { accept_stale_content: accept, ..Default::default() }, &sink,
        );
        let error = run(false).await.unwrap_err();
        assert!(error.contains("--accept-stale-content"), "{error}");
        let result = run(true).await;
        assert!(matches!(result, Ok(crate::plugins::types::DeployResult::NoPlugin { .. })), "{result:?}");
    }

    fn report_code(report: DeployReport) -> i32 {
        super::report(Ok(report), "site", false)
    }
}

/// `moss deploy --email` reaches the verification through `dispatch`, the
/// function every deploy goes through after the flags are parsed.
#[cfg(test)]
mod email_wiring_tests {
    use super::*;
    use crate::identity::verify_email::verify_email_tests::{Folder, Seta};

    async fn deploy(folder: &Folder, email: Option<&str>) -> Result<DeployReport, String> {
        std::fs::create_dir_all(folder.dir.path().join(".moss")).unwrap();
        std::fs::write(folder.dir.path().join(".moss/config.toml"), "environment = \"staging\"\n").unwrap();
        let site = folder.dir.path().join("_site");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.html"), "<p>hi</p>").unwrap();
        let flags = DeployFlags {
            email: email.map(str::to_string),
            overwrite_newer: true,
            ..DeployFlags::default()
        };
        dispatch(
            folder.dir.path(),
            &DeployRoute::Prebuilt { dir: site },
            &crate::cli::host::cli_host_ports,
            crate::build::PluginMode::Skip,
            &flags,
            &printing(),
        )
        .await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_first_publish_with_email_requests_the_link_then_registers_again() {
        let seta = Seta::start(1, "verified").await;
        let folder = Folder::new(&seta);

        let result = deploy(&folder, Some("ada@example.test")).await;

        assert_eq!(seta.count("POST /auth/request"), 1, "{result:?}");
        assert_eq!(seta.count_exact("POST /api/sites"), 2, "refused once, registered on the retry: {result:?}");
        assert!(result.is_ok(), "the retry publishes: {result:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_publish_records_the_account_its_key_now_belongs_to_and_leaves_the_key() {
        let seta = Seta::start(1, "verified").await;
        let folder = Folder::new(&seta);
        std::fs::write(folder.dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"my-site\"\n").unwrap();
        let key_file = folder.dir.path().join(".moss/identity/secret-key");
        let key_before = (std::fs::read(&key_file).unwrap(), std::fs::metadata(&key_file).unwrap().modified().unwrap());
        *seta.account.lock().unwrap() = Some("moved@example.test");

        let _ = deploy(&folder, None).await;

        assert_eq!(crate::identity::Identity::read_email(folder.dir.path()).as_deref(), Some("moved@example.test"));
        assert_eq!(seta.count_exact("GET /auth/whoami"), 1, "one whoami per publish");
        let key_after = (std::fs::read(&key_file).unwrap(), std::fs::metadata(&key_file).unwrap().modified().unwrap());
        assert_eq!(key_after, key_before, "the refresh never rewrites the secret key");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_fresh_key_on_an_existing_site_is_signed_in_with_email_and_only_with_it() {
        let seta = Seta::start(1, "verified").await;
        let folder = Folder::new(&seta);
        std::fs::write(folder.dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"my-site\"\n").unwrap();

        let _ = deploy(&folder, None).await;
        assert_eq!(seta.count("POST /auth/request"), 0, "no link without --email");

        let _ = deploy(&folder, Some("ada@example.test")).await;
        assert_eq!(seta.count("POST /auth/request"), 1, "the unlinked key is signed in");

        *seta.account.lock().unwrap() = Some("ada@example.test");
        let _ = deploy(&folder, Some("ada@example.test")).await;
        assert_eq!(seta.count("POST /auth/request"), 1, "a linked key is never re-sent a link");
    }

    /// A new folder published to a site its account already owns: the link
    /// names that site, so opening it signs the key in to the site and the retry
    /// publishes, instead of being refused again with the same sign-in advice.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_first_publish_signs_in_naming_the_site_it_publishes_to() {
        let seta = Seta::start(1, "verified").await;
        let folder = Folder::new(&seta);

        let _ = deploy(&folder, Some("ada@example.test")).await;

        let target = crate::deploy::derive_site_id_from_root(&crate::vault::paths::VaultRoot::resolve(folder.dir.path()));
        assert_eq!(seta.sign_in_bodies()[0]["site_id"], target.as_str());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_first_publish_without_email_requests_nothing_and_names_the_flag() {
        let seta = Seta::start(1, "verified").await;
        let folder = Folder::new(&seta);

        let err = deploy(&folder, None).await.expect_err("the key is not verified");

        assert_eq!(seta.count("POST /auth/request"), 0);
        assert!(err.contains("--email <address>"), "{err}");
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[tokio::test]
    async fn deploy_profile_prebuilt_dry_run_is_one_record() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".moss")).unwrap();
        std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"dry-profile\"\n").unwrap();
        crate::identity::Identity::generate().unwrap().save(dir.path()).unwrap();
        let site = dir.path().join("output");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(site.join("index.html"), "<p>private-page</p>").unwrap();
        let route = DeployRoute::Prebuilt { dir: site };
        let result = dispatch(dir.path(), &route, &crate::cli::host::cli_host_ports,
            crate::build::PluginMode::Skip, &DeployFlags { dry_run: true, ..Default::default() },
            &crate::deploy::progress::silent()).await;
        assert!(matches!(result, Ok(DeployReport::DryRun(_))), "{result:?}");
        let text = std::fs::read_to_string(crate::moss_paths::MossPaths::new(dir.path()).profile()).unwrap();
        assert_eq!(text.lines().count(), 1);
        let row: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(row["kind"], "deploy");
        assert_eq!(row["outcome"], "ok");
        assert_eq!(row["target"], "prebuilt");
        assert_eq!(row["dry_run"], true);
        assert_eq!(row["counts"]["manifest_files"], 1);
        assert_eq!(row["counts"]["body_requests"], 0);
        assert!(!text.contains("private-page"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deploy_profile_build_and_plugin_dry_runs_reference_the_build() {
        let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        for route in [DeployRoute::Hosted, DeployRoute::Plugin { id: "fixture".into() }] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join(".moss")).unwrap();
            std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"dry-profile\"\n").unwrap();
            crate::identity::Identity::generate().unwrap().save(dir.path()).unwrap();
            std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\nuid: dry-home\n---\n\nHello.\n").unwrap();
            let result = dispatch(dir.path(), &route, &crate::cli::host::cli_host_ports,
                crate::build::PluginMode::Skip, &DeployFlags { dry_run: true, ..Default::default() },
                &crate::deploy::progress::silent()).await;
            let Ok(DeployReport::DryRun(dry)) = result else { panic!("{result:?}") };
            let text = std::fs::read_to_string(crate::moss_paths::MossPaths::new(dir.path()).profile()).unwrap();
            let rows: Vec<serde_json::Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            let deploys: Vec<_> = rows.iter().filter(|r| r["kind"] == "deploy").collect();
            assert_eq!(deploys.len(), 1);
            let row = deploys[0];
            assert_eq!(row["target"], crate::deploy::profile::target(&route));
            assert_eq!(row["dry_run"], true);
            assert_eq!(row["counts"]["pages_added"], dry.change_set.added);
            assert_eq!(row["counts"]["pages_edited"], dry.change_set.edited);
            assert_eq!(row["counts"]["pages_deleted"], dry.change_set.deleted);
            assert_eq!(row["counts"]["pages_restyled"], dry.change_set.restyled);
            assert!(rows.iter().any(|b| b["kind"] == "build" && b["started_at"] == row["build_started_at"]));
        }
    }

}
