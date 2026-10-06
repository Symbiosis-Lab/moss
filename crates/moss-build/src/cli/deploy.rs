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
    /// `--dry-run`: build as a deploy would, say what it would do, send
    /// nothing and record nothing.
    pub dry_run: bool,
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
                "--accept-removals" => flags.accept_removals = true,
                "--dry-run" => flags.dry_run = true,
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
        if flags.dry_run && flags.accept_removals {
            return Err(
                "error: --dry-run records nothing, so it cannot accept removals. Drop --accept-removals to see \
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
    runtime.block_on(dispatch(folder, route, host_ports, crate::build::PluginMode::Blocking, flags, &sink))
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
    // Before the route's own send: a dry run is the same build with
    // everything after the gate left out, so it never reaches the match.
    if flags.dry_run {
        return match route {
            DeployRoute::Prebuilt { .. } => Err(
                "--dry-run needs a build to describe, and a prebuilt folder is uploaded as it is."
                    .to_string(),
            ),
            _ => crate::deploy::dry_run::run_dry_run(folder, route, flags.site_id.as_deref(), host_ports, plugins)
                .await
                .map(|dry| DeployReport::DryRun(Box::new(dry))),
        };
    }
    match route {
        DeployRoute::Prebuilt { dir } => {
            crate::deploy::prebuilt::run_prebuilt_deploy(
                folder,
                dir,
                flags.site_id.as_deref(),
                flags.overwrite_newer,
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
                flags.accept_removals,
                sink,
            )
            .await
            .and_then(|published| {
                crate::deploy::plugin_push::report_for(&published)
            })
        }
    }
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
        .filter(|r| r.reason == RemovalReason::AuthorRemoved)
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
        RemovedAddress { path: path.into(), reason, moved_to: None, source: source.map(Into::into) }
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

    /// A dry run records nothing, so it cannot also record an acceptance.
    #[test]
    fn a_dry_run_cannot_accept_removals() {
        let err = parse(&["site", "--dry-run", "--accept-removals"]).unwrap_err();
        assert!(err.starts_with("error: --dry-run records nothing"), "{err}");
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

    struct Fixture {
        dir: tempfile::TempDir,
        connections: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _env: std::sync::MutexGuard<'static, ()>,
        prev_url: Option<String>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            match self.prev_url.take() {
                Some(u) => std::env::set_var("MOSS_SETA_URL", u),
                None => std::env::remove_var("MOSS_SETA_URL"),
            }
        }
    }

    impl Fixture {
        /// A one-page folder; `lost_feed` adds a record of a publish that
        /// served `legacy-feed.xml`. The hosting server is a listener that records
        /// whether anything connected.
        fn new(lost_feed: bool) -> Self {
            let env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/hosted");
            std::fs::create_dir_all(&base).unwrap();
            let dir = tempfile::Builder::new().prefix("dry").tempdir_in(&base).unwrap();
            std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
            std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
            std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_id = \"dry-test\"\n").unwrap();
            // A registered site has a signing key; a dry run only checks it is there.
            std::fs::create_dir_all(dir.path().join(".moss/identity")).unwrap();
            std::fs::write(dir.path().join(".moss/identity/secret-key"), "k").unwrap();
            std::fs::write(dir.path().join(".moss/identity/public.json"), "{}").unwrap();
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
            // Counts connections and hangs up on each, so a deploy that does
            // reach for the server fails fast instead of waiting on it.
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (c, st) = (connections.clone(), stop.clone());
            std::thread::spawn(move || {
                while !st.load(std::sync::atomic::Ordering::Relaxed) {
                    match listener.accept() {
                        Ok(_) => {
                            c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                    }
                }
            });
            let prev_url = std::env::var("MOSS_SETA_URL").ok();
            std::env::set_var("MOSS_SETA_URL", format!("http://{addr}"));
            Self { dir, connections, stop, _env: env, prev_url }
        }

        async fn run(&self, dry_run: bool) -> Result<DeployReport, String> {
            let flags = DeployFlags { dry_run, ..DeployFlags::default() };
            dispatch(self.dir.path(), &DeployRoute::Hosted, &crate::cli::host::cli_host_ports, crate::build::PluginMode::Skip, &flags, &printing()).await
        }

        fn contacted_the_server(&self) -> bool {
            std::thread::sleep(std::time::Duration::from_millis(100));
            self.connections.load(std::sync::atomic::Ordering::Relaxed) > 0
        }

        fn record_bytes(&self) -> Vec<u8> {
            let path = crate::build::manifest::published_record::path_for(&MossPaths::new(self.dir.path()), "moss:dry-test");
            std::fs::read(path).unwrap_or_default()
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_that_a_real_deploy_would_refuse_exits_non_zero_and_sends_nothing() {
        let fx = Fixture::new(true);
        let before = fx.record_bytes();

        let report = fx.run(true).await.expect("a dry run reports, it does not fail");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.refusal.as_deref().is_some_and(|r| r.contains("/legacy-feed.xml")), "{dry:?}");
        assert_eq!(report_code(report), 1);
        assert!(!fx.contacted_the_server(), "a dry run must not contact the hosting server");
        assert_eq!(fx.record_bytes(), before, "a dry run must not write a published record");
        let key = fx.dir.path().to_string_lossy().to_string();
        assert!(
            crate::system::build_records::records().accepted_removals(&key).is_empty(),
            "a dry run must not record an acceptance"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_a_real_deploy_would_not_refuse_exits_zero_and_sends_nothing() {
        let fx = Fixture::new(false);

        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.refusal.is_none(), "{dry:?}");
        assert!(matches!(dry.transfer, crate::deploy::dry_run::Transfer::NoRecord), "no record, so there is nothing to count against: {dry:?}");
        assert_eq!(report_code(report), 0);
        assert!(!fx.contacted_the_server());
        assert!(fx.record_bytes().is_empty(), "nothing published, so no record");
    }

    /// A folder that never published: no `site_id`, and `moss env` run or not.
    fn unpublished(environment: bool) -> Fixture {
        let fx = Fixture::new(false);
        std::fs::remove_file(fx.dir.path().join(".moss/state.toml")).unwrap();
        std::fs::remove_dir_all(fx.dir.path().join(".moss/identity")).unwrap();
        if environment {
            std::fs::write(fx.dir.path().join(".moss/config.toml"), "environment = \"staging\"\n").unwrap();
        }
        fx
    }

    /// Where a deploy would fail before building, for a reason that needs
    /// nothing from the server, the dry run fails too, with the same words.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_folder_with_no_site_and_no_environment_fails_the_dry_run_as_a_deploy_would() {
        let fx = unpublished(false);
        let err = fx.run(true).await.expect_err("a deploy would say this folder has no site yet");
        assert!(err.contains("has no site yet") && err.contains("moss env"), "{err}");
        assert!(!fx.contacted_the_server());
    }

    /// A deploy creates a signing key when the site has none, so the dry run
    /// says that rather than refusing, and creates nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_registered_site_with_no_key_is_told_a_deploy_would_create_one() {
        let fx = Fixture::new(false);
        std::fs::remove_dir_all(fx.dir.path().join(".moss/identity")).unwrap();
        let report = fx.run(true).await.expect("a deploy would go on and create the key");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };
        assert!(dry.would_create_key && dry.refusal.is_none(), "{dry:?}");
        assert!(dry.render().contains("would create a new one"), "{}", dry.render());
        assert!(!fx.dir.path().join(".moss/identity").exists(), "the dry run must not create it");
    }

    /// An old identity keeps its private key inside `public.json`; a deploy
    /// migrates it and signs with it, so it is not "no key" and a dry run must
    /// not say a new one would be created (a new key is a different owner).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_legacy_identity_with_its_key_in_public_json_is_not_reported_as_missing() {
        let fx = Fixture::new(false);
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
        let fx = Fixture::new(false);
        let future = crate::config::migrations::CURRENT_VERSION + 1;
        std::fs::write(fx.dir.path().join(".moss/config.toml"), format!("schema_version = {future}\n")).unwrap();
        assert!(fx.run(true).await.is_err());
    }

    /// A first publish registers a site and creates a key; the dry run does
    /// neither, and says what it would do.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_first_publish_dry_run_registers_nothing_and_creates_no_key() {
        let fx = unpublished(true);
        let report = fx.run(true).await.expect("a dry run reports");
        let DeployReport::DryRun(dry) = &report else { panic!("expected a dry run, got {report:?}") };

        assert!(dry.would_register.is_some(), "{dry:?}");
        assert!(dry.render().contains("would register the site"), "{}", dry.render());
        assert!(!fx.dir.path().join(".moss/identity").exists(), "no key may be created");
        assert!(!fx.dir.path().join(".moss/state.toml").exists(), "no site may be registered");
        assert!(!fx.contacted_the_server());
    }

    /// A prebuilt folder is uploaded as it is: there is no build to describe.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dry_run_of_a_prebuilt_folder_is_a_usage_error() {
        let fx = Fixture::new(false);
        let flags = DeployFlags { dry_run: true, ..DeployFlags::default() };
        let route = DeployRoute::Prebuilt { dir: fx.dir.path().join("_site") };
        let err = dispatch(fx.dir.path(), &route, &crate::cli::host::cli_host_ports, crate::build::PluginMode::Skip, &flags, &printing())
            .await
            .expect_err("nothing to build");
        assert!(err.starts_with("--dry-run needs a build"), "{err}");
        assert!(!fx.contacted_the_server());
    }

    /// The plugin route reads the same gate: refused without the flag, and
    /// past it with the flag (this folder names no deploy plugin, so what
    /// comes back after the gate is the plugin route's own "no plugin").
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_plugin_route_honours_accept_removals() {
        let fx = Fixture::new(true);
        let sink = crate::deploy::progress::silent();
        let run = |accept| {
            crate::deploy::plugin_push::run_plugin_deploy(
                fx.dir.path(),
                &crate::cli::host::cli_host_ports,
                crate::build::PluginMode::Skip,
                accept,
                &sink,
            )
        };
        let refused = run(false).await.expect_err("an unexplained removal stops a plugin publish too");
        assert!(refused.contains("/legacy-feed.xml"), "{refused}");

        let past = run(true).await;
        assert!(!matches!(&past, Err(e) if e.contains("legacy-feed.xml")), "accepted, so not the gate: {past:?}");
    }

    fn report_code(report: DeployReport) -> i32 {
        super::report(Ok(report), "site")
    }
}
