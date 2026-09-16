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
        _home_page_entry: Option<&str>,
        _page_entries: &std::collections::HashMap<String, String>,
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
    sealed: std::sync::Arc<std::sync::Mutex<Option<SealedManifest>>>,
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
    let sealed: std::sync::Arc<std::sync::Mutex<Option<SealedManifest>>> =
        std::sync::Arc::default();
    host.announcer = std::sync::Arc::new(CapturingAnnouncer {
        inner: host.announcer.clone(),
        sealed: sealed.clone(),
    });
    sealed
}

/// One headless build and what it sealed. The hosted deploy, the plugin
/// deploy and `moss history` all build before they act; this is the build.
/// Each caller keeps its own session guard (the session must outlive the
/// action, not just the build) and decides when to `require_sealed`.
pub(crate) async fn build_and_seal(
    root: &crate::vault::paths::VaultRoot,
    mut host: crate::build::HostPorts,
    plugins: crate::build::PluginMode,
) -> Result<Option<SealedManifest>, String> {
    let sealed_slot = capture_seal(&mut host);
    let message = crate::build::run_pipeline(crate::build::PipelineConfig {
        root: root.clone(),
        progress: crate::build::stdout_sink(),
        plugins,
        watch: false,
        start_server: false,
        host,
        trigger: crate::build::BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        live_port: None,
        admission_epoch: None,
    })
    .await
    .map_err(|e| format!("build failed: {e}"))?;
    // Printing DRAINS the problem counter, so a publish that skipped this
    // would upload a site with N build problems in complete silence. It also
    // waits out any ui-bound background work, which is a no-op headless today
    // and a guarantee rather than a coincidence.
    crate::build::cli_output::finish_cli_build(&message, root.as_str(), false).await;
    let taken = sealed_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    Ok(taken)
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
        "Deploy artifacts are not sealed yet. Rebuild the folder and retry publishing.".to_string()
    })
}

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
            trigger: crate::build::BuildTrigger::Full,
            exits_after_build: true,
            site_url_override: None,
            server_port: None,
            live_port: None,
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
}
