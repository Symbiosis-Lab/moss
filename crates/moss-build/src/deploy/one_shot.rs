//! What a publish that builds its own site does around the build.
//!
//! Three things, shared by the two drivers that run one — [`super::push`] for
//! moss hosting and [`super::plugin_push`] for a deploy plugin. A one-shot has
//! no watcher to have sealed a manifest for it and no app to hold the
//! generation still, so both drivers wrap the host's announcer to catch the
//! seal as it happens, and both answer the app-shaped `DeployPorts` questions
//! with nothing.
//!
//! It lived in `push.rs` until the second driver arrived at track P slice P2b.
//! The split is where the sharing already was: nothing here is moss-hosted,
//! and `plugin_push` reaching into `push` for it would have said the opposite.

use std::path::Path;

use crate::build::manifest::SealedManifest;

/// What a publish needs from a process with no window: nothing.
///
/// Every method on [`DeployPorts`] exists because the app answers it with
/// behaviour a terminal has no equivalent of, and this is the other side of
/// that sentence written out. The pin guards a generation against a concurrent
/// build's garbage collector; a headless publish built the generation itself,
/// synchronously, and no second build is running. `after_landing` re-derives a
/// change set for a frontend that is not there. `ensure_domain_ready` drives
/// the app's domain orchestrator, which owns DNS state a one-shot process
/// cannot usefully hold — a custom domain is readied when the app next opens
/// the folder, and the publish reports the mosspub URL meanwhile.
///
/// Empty rather than `Option`-threaded: an `Option<&dyn DeployPorts>` would put
/// four `if let` branches inside the publish body to express what these four
/// empty bodies express once.
pub(crate) struct HeadlessDeployPorts;

#[async_trait::async_trait]
impl crate::build::ports::deploy::DeployPorts for HeadlessDeployPorts {
    fn pin_generation_raw(&self, _generation_id: &str) {}
    fn unpin_generation(&self, _generation_id: &str) {}
    async fn after_landing(&self, _folder: &Path, _target: &str, _summary: &crate::deploy::change_record::PageChangeSummary) {}
    async fn ensure_domain_ready(&self, _folder_path: &str) {}
    async fn begin_moss_verification(
        &self,
        _identity: &crate::identity::Identity,
        _environment: crate::config::environment::HostingEnvironment,
        _site_id: &str,
        _folder_path: &str,
        _generation_id: &str,
        _summary: &crate::deploy::change_record::PageChangeSummary,
    ) {
    }
}

/// Keeps the sealed manifest the build hands over, and passes every
/// announcement through to whoever the host had.
///
/// [`SealAnnouncer::adopt_sealed`]'s contract is "hand the manifest to whoever
/// holds it for deploy", and in the app that holder is `AppState`. A one-shot
/// publish holds it in a local, which is what this is. Wrapping rather than
/// replacing matters: the host's announcer still logs the promotion and still
/// publishes to the SSE carrier under `--serve`, so capturing the manifest
/// costs the caller nothing else.
///
/// This is why [`super::push::run_hosted_deploy`] needs no new seam. The alternative — a
/// `run_pipeline` that returns its manifest — would change the signature every
/// build in the codebase calls, to serve the one caller that publishes.
pub(crate) struct CapturingAnnouncer {
    inner: std::sync::Arc<dyn crate::build::ports::announcer::SealAnnouncer>,
    accept_removals: bool,
    sealed: std::sync::Arc<std::sync::Mutex<Option<SealedManifest>>>,
    /// What the seal said publishing would change — see [`Captured`].
    change_set: std::sync::Arc<std::sync::Mutex<Option<crate::build::manifest::change_set::ChangeSet>>>,
}

#[async_trait::async_trait]
impl crate::build::ports::announcer::SealAnnouncer for CapturingAnnouncer {
    fn promoted(&self, generation_id: &str) {
        self.inner.promoted(generation_id);
    }

    async fn adopt_sealed(&self, sealed: SealedManifest) {
        *self.sealed.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(sealed.clone());
        self.inner.adopt_sealed(sealed).await;
    }

    async fn publish_change_set(
        &self,
        project_root: &Path,
        change_set: Option<crate::build::manifest::change_set::ChangeSet>,
    ) {
        *self.change_set.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = change_set.clone();
        // The seal has recorded its removals; accept them before the host announces them.
        if self.accept_removals && change_set.is_some() {
            crate::system::build_records::records().accept_all_pending_removals(&project_root.to_string_lossy());
        }
        self.inner.publish_change_set(project_root, change_set).await;
    }
}

/// Wrap `host`'s announcer so the manifest the build seals lands in the slot
/// this returns, and hand the slot back.
///
/// Both one-shot publish drivers do exactly this and neither may skip it: the
/// seal is the only moment the manifest exists, and `run_pipeline` does not
/// return it. Wrapping rather than replacing is the load-bearing half — the
/// host's own announcer still logs the promotion and still publishes to the
/// SSE carrier under `--serve`.
pub(crate) fn capture_seal(
    host: &mut crate::build::HostPorts,
) -> std::sync::Arc<std::sync::Mutex<Option<SealedManifest>>> {
    capture(host).sealed
}

/// What the build's seal handed the announcer: the manifest, and the change
/// set it classified against the last publish (`--dry-run` reads that instead
/// of classifying again).
pub(crate) struct Captured {
    pub sealed: std::sync::Arc<std::sync::Mutex<Option<SealedManifest>>>,
    pub change_set:
        std::sync::Arc<std::sync::Mutex<Option<crate::build::manifest::change_set::ChangeSet>>>,
}

/// [`capture_seal`] with the change set too.
pub(crate) fn capture(host: &mut crate::build::HostPorts) -> Captured {
    capture_accepting(host, false)
}

fn capture_accepting(host: &mut crate::build::HostPorts, accept_removals: bool) -> Captured {
    let captured = Captured { sealed: Default::default(), change_set: Default::default() };
    host.announcer = std::sync::Arc::new(CapturingAnnouncer {
        inner: host.announcer.clone(),
        accept_removals,
        sealed: captured.sealed.clone(),
        change_set: captured.change_set.clone(),
    });
    captured
}

/// One headless build and what it sealed. The hosted deploy, the plugin
/// deploy and `moss history` all build before they act; this is the build.
/// Each caller keeps its own session guard (the session must outlive the
/// action, not just the build) and decides when to `require_sealed`.
/// The build every one-shot publish route runs before its upload: build and
/// seal the folder, then apply what the person accepted to that seal.
pub(crate) async fn build_for_publish(
    folder: &std::path::Path,
    host: crate::build::HostPorts,
    plugins: crate::build::PluginMode,
    overrides: &crate::deploy::push::PublishOverrides,
    sink: &std::sync::Arc<dyn crate::deploy::progress::DeploySink>,
) -> Result<Option<SealedManifest>, String> {
    sink.stage(crate::deploy::progress::DeployStage::Preparing, 0, 0, "Building the site...");
    let root = crate::vault::paths::VaultRoot::resolve(folder);
    let sealed = build_and_seal(&root, host, plugins, overrides.accept_removals).await?;
    overrides.accept(folder, sealed.as_ref());
    Ok(sealed)
}

pub(crate) async fn build_and_seal(
    root: &crate::vault::paths::VaultRoot,
    host: crate::build::HostPorts,
    plugins: crate::build::PluginMode,
    accept_removals: bool,
) -> Result<Option<SealedManifest>, String> {
    Ok(build_and_describe(root, host, plugins, accept_removals).await?.0)
}

/// [`build_and_seal`], and the change set the seal computed along the way.
pub(crate) async fn build_and_describe(
    root: &crate::vault::paths::VaultRoot,
    mut host: crate::build::HostPorts,
    plugins: crate::build::PluginMode,
    accept_removals: bool,
) -> Result<(Option<SealedManifest>, Option<crate::build::manifest::change_set::ChangeSet>), String> {
    let _phase = crate::build::phase::PhaseTrace::start("build");
    let captured = capture_accepting(&mut host, accept_removals);
    let message = crate::build::run_pipeline(crate::build::PipelineConfig {
        root: root.clone(),
        progress: crate::build::stdout_sink(),
        plugins,
        watch: false,
        start_server: false,
        host,
        // Pages nothing changed are carried from the last seal; with nothing
        // changed at all, the build hands that seal over from its entry.
        trigger: crate::build::BuildTrigger::Unobserved,
        reason: Some(crate::build::BuildReason::Publish),
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        live_port: None,
        rename_pairs: Vec::new(),
        admission_epoch: None,
    })
    .await
    .map_err(|e| format!("build failed: {e}"))?;
    // Printing DRAINS the problem counter, so a publish that skipped this
    // would upload a site with N build problems in complete silence. It also
    // waits out any ui-bound background work, which is a no-op headless today
    // and a guarantee rather than a coincidence.
    let unwritten = crate::system::build_records::records().last_build_failed(root.as_str());
    crate::build::cli_output::finish_cli_build(&message, root.as_str(), false, unwritten);
    crate::build::cli_output::wait_for_background_tasks(root.as_str()).await;
    let taken = captured.sealed.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    let change_set = captured.change_set.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    if let Some(set) = &change_set { super::profile::pages(set); }
    if let Some(sealed) = &taken { super::profile::count("manifest_files", sealed.files().len() as u64); }
    Ok((taken, change_set))
}

/// A sealed manifest of the tree as it stands right now, built on the spot —
/// what a process with no `AppState` has instead of the live one the app keeps.
///
/// The migration checker uses this to compare output before and after an edit.
/// History saves and restores read the disk directly and do not call this.
/// Plugin processing is blocking so the comparison includes plugin content.
/// This does not register a folder session or replace a running preview.
pub async fn build_sealed_now(
    root: &crate::vault::paths::VaultRoot,
) -> Result<SealedManifest, String> {
    let host = crate::cli::host::cli_host_ports(root.as_str());
    require_sealed(build_and_seal(root, host, crate::build::PluginMode::Blocking, false).await?)
}

/// The one sentence every publish driver refuses a sealless publish with.
///
/// An empty slot is never "no manifest, carry on". In a one-shot `capture_seal`
/// fills it from `adopt_sealed`, which the seal tail reaches only when the
/// generation this build sealed actually became `current` — so an empty slot
/// means the promotion was withheld, superseded or failed, and `current` still
/// points at the PREVIOUS build. Moss hosting uploads FROM the manifest and
/// could only fail here anyway; a plugin uploads the generation directory and
/// would happily ship that older tree while reporting the new build as
/// published.
///
/// Three callers, and one of them is not a one-shot: the app's two publish
/// routes read the slot out of `AppState`, where a build is not the only thing
/// that can leave it empty — a materialize can fail, and a folder switch leaves
/// ANOTHER site's seal behind (`AppState::current_sealed_manifest` documents
/// that gap). So the sentence cannot name "this build": on the app there may
/// have been no build in this session at all. It names the next step instead,
/// which is the same on all three routes — build the folder again, then
/// publish. One owner so the drivers cannot drift into telling an author three
/// different things about the same empty slot.
pub fn require_sealed<T>(sealed: Option<T>) -> Result<T, String> {
    sealed.ok_or_else(|| {
        super::profile::refusal("unsealed");
        "Deploy artifacts are not sealed yet. Rebuild the folder and retry publishing.".to_string()
    })
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-shot build hands its sealed manifest to the announcer the host
    /// installed — the single fact [`super::super::push::run_hosted_deploy`] is built on.
    ///
    /// Driven through `run_pipeline` rather than by calling
    /// [`CapturingAnnouncer`] by hand, because the wrapper was never the part
    /// that could break. It shipped correct and unreachable: the inline seal
    /// arm hardcoded `LogAnnouncer` and threw `host.announcer` away, so every
    /// hosted deploy failed with "the build sealed no generation" while a
    /// unit test of the wrapper passed. A test that constructs the collaborator
    /// it is testing cannot see that nobody calls it.
    ///
    /// Cheap despite the pipeline: a two-file vault, no network, no `site_id`,
    /// no publish. The assertion is about the seal, not about seta.
    /// Multi-thread because the pipeline's blocking pool is, and so is every
    /// real caller: `cli::deploy` builds its own multi-thread runtime for
    /// exactly this reason.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_one_shot_build_hands_its_manifest_to_the_host_announcer() {
        // Test constraint: `register_session` drains every session in the
        // process-global registry, so another test registering one while this
        // build runs shuts this build's session down, its promotion is
        // withheld or superseded, and the slot stays empty. Tests that
        // register sessions serialize on this lock.
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // `../../` deliberately: the workspace `/target`, which is gitignored.
        // `crates/moss-build/target/` is not, and in a shared checkout that is
        // one peer session's `git add -A` away from being committed — the
        // near-miss `.gitignore:50`'s own comment was written for.
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/hosted");
        std::fs::create_dir_all(&base).expect("create temp base");
        let tmp = tempfile::Builder::new()
            .prefix("seal")
            .tempdir_in(&base)
            .expect("create vault dir");
        std::fs::write(tmp.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n")
            .expect("write index.md");
        let folder = tmp.path().to_string_lossy().to_string();

        let _session = crate::system::folder_session::register_session(&folder);
        let mut host = crate::cli::host::cli_host_ports(&folder);
        let slot = capture_seal(&mut host);

        crate::build::run_pipeline(crate::build::PipelineConfig {
            root: crate::vault::paths::VaultRoot::resolve(tmp.path()),
            progress: crate::build::stdout_sink(),
            plugins: crate::build::PluginMode::Skip,
            watch: false,
            start_server: false,
            host,
            trigger: crate::build::BuildTrigger::Full, reason: None,
            exits_after_build: true,
            site_url_override: None,
            server_port: None,
            live_port: None,
            rename_pairs: Vec::new(),
            admission_epoch: None,
        })
        .await
        .expect("the build must succeed");

        let sealed = slot.lock().unwrap().take();
        assert!(
            sealed.is_some(),
            "a publish driver ships what lands here; None is a deploy that cannot start"
        );
        assert!(
            !sealed.expect("checked").files().is_empty(),
            "the manifest that reaches the publish is the one that was sealed, not an empty twin"
        );
    }

    fn build_counts(root: &Path) -> serde_json::Value {
        let profile = std::fs::read_to_string(crate::moss_paths::MossPaths::new(root).profile()).unwrap();
        profile.lines().rev().map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["kind"] == "build").unwrap()["counts"].clone()
    }

    async fn deploy_build(root: &Path) -> SealedManifest {
        deploy_build_with(root, crate::build::PluginMode::Skip).await
    }

    async fn deploy_build_with(root: &Path, plugins: crate::build::PluginMode) -> SealedManifest {
        let root = crate::vault::paths::VaultRoot::resolve(root);
        require_sealed(build_and_seal(&root, crate::cli::host::cli_host_ports(root.as_str()),
            plugins, false).await.unwrap()).unwrap()
    }

    /// A sign-in or sign-out after the build leaves the generation behind the
    /// account an identity tag reads; the publish's own record does not.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_account_change_after_the_build_moves_the_state() {
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = two_page_site("---\ntitle: About\n---\n\nAbout.\n");
        let folder = dir.path().to_string_lossy().to_string();
        let built = deploy_build(dir.path()).await;
        assert!(!built.state_moved(dir.path()), "a fresh build is current");
        let binding = crate::plugins::manager::accounts::AccountBinding { id: "acct:1".into(), name: None, host: None };
        crate::plugins::manager::accounts::bind_account(&folder, "social", &binding).unwrap();
        assert!(built.state_moved(dir.path()), "a sign-in moved it");
        let signed_in = deploy_build(dir.path()).await;
        assert!(!signed_in.state_moved(dir.path()), "a rebuild catches up");
        crate::plugins::manager::accounts::unbind_account(&folder, "social").unwrap();
        assert!(signed_in.state_moved(dir.path()), "a sign-out moved it");
        let signed_out = deploy_build(dir.path()).await;
        assert!(!signed_out.state_moved(dir.path()));
        let state = dir.path().join(".moss/state.toml");
        std::fs::remove_file(&state).unwrap();
        assert!(signed_out.state_moved(dir.path()), "a removed state file moved it");
        std::fs::create_dir(&state).unwrap();
        assert!(signed_out.state_moved(dir.path()), "an unreadable one moved it");
    }

    fn two_page_site(about: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
        std::fs::write(dir.path().join("about.md"), about).unwrap();
        dir
    }

    /// Every record the publish gate reads starts empty in a new process.
    fn forget_gate_records(folder: &str) {
        crate::system::build_records::records().forget_folder(folder);
    }

    /// A deploy with nothing changed since the last build ends at its entry
    /// and hands over the generation that build sealed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_deploy_with_nothing_changed_hands_over_the_kept_seal() {
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = two_page_site("---\ntitle: About\n---\n\nAbout us.\n");
        let folder = dir.path().to_string_lossy().to_string();
        let _session = crate::system::folder_session::register_session(&folder);
        // The first build writes files the second reads, so the second renders.
        deploy_build(dir.path()).await;
        let first = deploy_build(dir.path()).await;
        forget_gate_records(&folder);
        let second = deploy_build(dir.path()).await;
        let counts = build_counts(dir.path());
        assert_eq!(counts["unchanged_at_entry"], 1, "{counts}");
        assert_eq!(second.generation_id(), first.generation_id());
        assert_eq!(second.files(), first.files(), "a stopped deploy uploads the same site");
        assert_eq!(crate::deploy::publish_refusal(&folder).map(|r| r.kind), None);
        assert!(crate::deploy::refuse_unresolved_inputs(&second).is_ok(), "the kept seal brings its read evidence");
        assert!(crate::deploy::refuse_foreign_inputs(&second, dir.path()).is_ok());
        assert!(matches!(crate::system::build_records::records().build_end(&folder), Some(crate::system::build_records::BuildEnd::Publishable)));
    }

    /// A stopped deploy refuses what the kept seal found, in a process that
    /// never rendered the page.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stopped_deploy_refuses_the_missing_reference_the_kept_seal_found() {
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = two_page_site("---\ntitle: About\n---\n\n![Team](team.png)\n");
        let folder = dir.path().to_string_lossy().to_string();
        let _session = crate::system::folder_session::register_session(&folder);
        deploy_build(dir.path()).await;
        deploy_build(dir.path()).await;
        let found = crate::system::build_records::records().publish_preflight(&folder).expect("the build installs its evidence");
        assert!(!found.missing_references.is_empty(), "fixture: team.png is missing");
        forget_gate_records(&folder);
        deploy_build(dir.path()).await;
        assert_eq!(build_counts(dir.path())["unchanged_at_entry"], 1);
        let handed = crate::system::build_records::records().publish_preflight(&folder).expect("the stop installs the kept evidence");
        assert_eq!(handed.missing_references, found.missing_references);
        assert!(matches!(crate::deploy::publish_refusal(&folder).map(|r| r.kind), Some(crate::deploy::RefusalKind::MissingReferences)));
    }

    /// A generation missing a file it names is not handed over: the deploy
    /// builds and seals a whole one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_kept_generation_missing_a_file_is_built_again() {
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = two_page_site("---\ntitle: About\n---\n\nAbout us.\n");
        let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
        deploy_build(dir.path()).await;
        let first = deploy_build(dir.path()).await;
        let page = crate::moss_paths::MossPaths::new(dir.path()).generation_dir(first.generation_id()).join("about/index.html");
        std::fs::remove_file(&page).unwrap();
        deploy_build(dir.path()).await;
        assert_ne!(build_counts(dir.path())["unchanged_at_entry"], 1);
        assert!(page.exists() || crate::moss_paths::MossPaths::new(dir.path()).current_ptr().join("about/index.html").exists(), "the deploy sealed a whole generation");
    }

    /// A manifest an older moss wrote has no evidence, and is never handed over.
    #[test]
    fn a_manifest_without_evidence_hands_nothing_over() {
        let inputs: crate::build::inputs::BuildInputs = serde_json::from_str(r#"{"moss":{},"env":{}}"#).unwrap();
        assert_eq!(inputs.evidence, None);
        let manifest = crate::types::content::SiteHashes { build_inputs: Some(inputs), ..Default::default() };
        assert!(SealedManifest::kept(manifest).is_none());
    }

    /// A kept seal answers for the folder that sealed it, not the one it is read in.
    #[test]
    fn a_kept_seal_is_refused_in_another_folder() {
        let (sealed_in, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let evidence = crate::build::inputs::SealEvidence { root: Some(sealed_in.path().to_path_buf()), ..Default::default() };
        let inputs = crate::build::inputs::BuildInputs { evidence: Some(evidence), ..Default::default() };
        let kept = SealedManifest::kept(crate::types::content::SiteHashes { build_inputs: Some(inputs), ..Default::default() }).unwrap();
        assert!(crate::deploy::refuse_foreign_inputs(&kept, sealed_in.path()).is_ok());
        assert!(crate::deploy::refuse_foreign_inputs(&kept, other.path()).is_err());
    }

    const COMMENT_TEXT: &str = "MOSS_MATTERS_TEST_PROFILE";

    /// A site whose process hook writes the comments `about` shows into
    /// `.moss/data/social/`.
    fn comment_site() -> tempfile::TempDir {
        std::env::remove_var(COMMENT_TEXT);
        let dir = two_page_site("---\ntitle: About\nuid: ab12cd34\ncomments: true\n---\n\nAbout us.\n");
        let plugin = dir.path().join(".moss/plugins/comment-writer");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(dir.path().join(".moss/config.toml"), "[services.comments]\nenabled = true\n").unwrap();
        for file in ["manifest.json", "main.bundle.js"] {
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins/comment-writer").join(file);
            std::fs::copy(fixture, plugin.join(file)).unwrap();
        }
        dir
    }

    /// Deploy until the hook has nothing new to write.
    async fn settle(root: &Path) {
        for _ in 0..3 {
            deploy_build_with(root, crate::build::PluginMode::Blocking).await;
        }
    }

    fn about_html(root: &Path, sealed: &SealedManifest) -> String {
        assert!(sealed.files().contains_key("about/index.html"));
        std::fs::read_to_string(crate::moss_paths::MossPaths::new(root).current_ptr().join("about/index.html")).unwrap()
    }

    /// A process hook that writes nothing new leaves the deploy free to carry.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_deploy_whose_process_hook_wrote_nothing_still_carries() {
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = comment_site();
        let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
        settle(dir.path()).await;
        let sealed = deploy_build_with(dir.path(), crate::build::PluginMode::Blocking).await;
        let counts = build_counts(dir.path());
        assert_eq!(counts["pages_rendered"], 0, "{counts}");
        assert!(counts["pages_carried"].as_u64().unwrap() >= 1, "{counts}");
        assert!(about_html(dir.path(), &sealed).contains("First reader comment"), "fixture: the page shows the hook's comment");
    }

    /// A process hook writes the comments a page shows into `.moss/data/social/`
    /// during the deploy's own build, after the build measured `.moss/`, and
    /// with nothing in the folder or the plugin's directory moving. The
    /// deploy ships the page with the new comment, not the carried old one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_deploy_ships_the_comments_its_process_hook_wrote() {
        let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = comment_site();
        let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
        settle(dir.path()).await;
        std::env::set_var(COMMENT_TEXT, "Second reader comment");
        let sealed = deploy_build_with(dir.path(), crate::build::PluginMode::Blocking).await;
        std::env::remove_var(COMMENT_TEXT);
        let counts = build_counts(dir.path());
        let written = std::fs::read_to_string(dir.path().join(".moss/data/social/comment-writer.json")).unwrap();
        assert!(written.contains("Second reader comment"), "fixture: the hook wrote the new comment: {written}");
        let html = about_html(dir.path(), &sealed);
        assert!(html.contains("Second reader comment"), "{counts}");
        assert!(!html.contains("First reader comment"), "{counts}");
    }
}
