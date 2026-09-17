//! One plugin-owned publish, from a folder to what the plugin reported.
//!
//! The third of moss's three publish drivers — [`super::prebuilt`] uploads a
//! directory another tool built, [`super::push`] builds and ships to moss's
//! own hosting, and this one builds and hands the result to whichever plugin
//! `[hooks] deploy` names. It is the last one that needed a window, and it
//! needed one only because nobody had written it: the plugin RUNTIME crossed
//! at ADR-076, so `HostPorts::plugins` already yields a manager whose
//! `execute_deploy` is pure moss-build (track P slice P2b, moss#989).
//!
//! Shaped like [`super::push::run_hosted_deploy`] on purpose — resolve, build,
//! gate, then an inner body over what the build produced — because the app's
//! `deploy_site_body` enters that inner body with a build a watcher already
//! made, and a driver that interleaved its resolution with its work could not
//! be entered twice. It does, since track P slice P3: below
//! [`run_plugin_deploy_inner`] there is one plugin publish in moss, and the
//! only difference between clicking Publish and typing `moss deploy` is which
//! preamble got the folder built.
//!
//! **What this route does NOT do.** It brings no stack up. The app preflights
//! the Tor stack before any bytes move (`system::stack_serving`, ADR-050) and
//! a one-shot does not; a terminal publish to a stopped OnionPress is refused
//! by the plugin itself, whose refusal is what tells the author to start it —
//! that wording is the plugin's, not moss's. And moss runs no probe of its own
//! here: the plugin's reachability poll is the whole verdict, printed verbatim
//! by [`report_for`]. The app's post-publish supervisor is what settles a
//! later verdict, and it reaches a terminal publish too — [`record_landing`]
//! persists the plugin's `metadata` on both binaries, and
//! `resume_publish_verification` derives its work from `metadata.generation`
//! (`stack_serving::verify::pending_verification`) while `record_publish`
//! deliberately never writes `last_verified_generation`, so opening the folder
//! in the app afterwards arms verification on what the terminal shipped
//! (track P slice P4).
//!
//! `resolve_publish_inputs` is deliberately absent. It is the seta
//! registration path, and calling it here would mint a moss-hosted site for a
//! folder that publishes to onionpress or GitHub Pages. A plugin target's
//! publish record is keyed by the bare method, with no site id.

use std::path::{Path, PathBuf};

use crate::build::manifest::SealedManifest;
use crate::config::deployment;
use crate::deploy::plugin_context::{
    build_deploy_context, deploy_failed, get_git_origin, refuses_subpath_pages,
};
use crate::deploy::progress::{self, DeployStage};
use crate::deploy::DeployReport;
use crate::moss_paths::MossPaths;
use crate::plugins::types::DeployResult;

/// A plugin publish as the result both binaries' terminals print.
///
/// Named and separate because the mapping is a decision, not a formatting
/// step: it decides which plugin outcomes are an exit code and a URL on
/// stdout and which are a failure. The one that cannot be carried is
/// success-with-no-address (see the `None` arm) — a terminal whose whole
/// success contract is the site address cannot report a publish that named
/// nowhere, while the frontend can render it as a toast. Its unit tests are
/// what pin the three arms.
///
/// The plugin's `message` is carried out whole. It is what the plugin
/// determined and moss did not — OnionPress's is the verdict of a ~20s
/// reachability poll — and it replaces the two counts this used to print. One
/// of those was fabricated (`files_removed: 0`: nothing tells moss what the
/// plugin deleted at the far end) and the other described moss's own
/// enumeration while being labelled "uploaded", which a plugin that ships a
/// diff makes false. The driver already states the rule for its progress
/// events: no counts, because a fabricated fraction is worse than none.
pub fn report_for(result: &DeployResult) -> Result<DeployReport, String> {
    match result {
        DeployResult::Completed { result } if result.success => match &result.deployment {
            Some(deployment) => Ok(DeployReport::Plugin {
                url: deployment.url.clone(),
                message: result.message.clone(),
            }),
            // The plugin says it worked and names nowhere it worked to. Not a
            // publish this command can report: stdout is the site address and
            // there is no address. The app's own body treats the same case as
            // success-with-nothing-to-record, which is the right answer for a
            // toast and the wrong one for `$(moss deploy .)`.
            None => Err("the deploy plugin reported success but named no published address"
                .to_string()),
        },
        DeployResult::Completed { result } => Err(result
            .message
            .clone()
            .unwrap_or_else(|| crate::infra::app_advisory::t("publish_failed"))),
        DeployResult::NoPlugin { message } => Err(message.clone()),
    }
}

/// Build the folder, then let its deploy plugin publish what the build sealed.
///
/// `host_ports` is a factory for the reason [`super::push::run_hosted_deploy`]
/// takes one: the ports are built after the folder session is registered, and
/// they are not `Clone`. `plugins` is [`crate::build::PluginMode::Blocking`]
/// from every caller that publishes — a plugin whose content arrives after the
/// deploy hook has read the generation has not been published, and there is no
/// next rebuild to pick it up.
pub async fn run_plugin_deploy(
    folder: &Path,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
    sink: &std::sync::Arc<dyn progress::DeploySink>,
) -> Result<DeployResult, String> {
    let folder_str = folder.to_string_lossy().to_string();

    sink.stage(
        DeployStage::Preparing,
        0,
        0,
        &crate::infra::app_advisory::t("preparing_to_publish"),
    );

    // The setup gate FIRST, before the build — a credential the target
    // declared and moss does not hold is knowable at t=0, and refusing after a
    // build spends minutes to say something that was already true (ADR-072).
    // This is the one route the gate can refuse: moss's own hosting has no
    // deploy plugin to be unset up.
    crate::deploy::publish_setup::refuse_publish(&folder_str)?;

    // The stage-write lock every concurrent-build path takes: `run_pipeline`
    // reaches for it, and so does the seal tail.
    let _session = crate::system::folder_session::register_session(&folder_str);

    sink.stage(DeployStage::Preparing, 0, 0, "Building the site...");

    let root = crate::vault::paths::VaultRoot::resolve(folder);
    let host = host_ports(&folder_str);
    // Taken before `host` moves into the build: this is the manager the
    // build's own hooks ran on, so the deploy hook shares its engine and its
    // already-initialized plugin instances.
    let managers = host.plugins.clone();

    // An empty slot is refused rather than shrugged at, by
    // `one_shot::require_sealed` inside the inner body — see its doc for why
    // the absence is a promotion failure and not a missing description.
    let sealed = super::one_shot::build_and_seal(&root, host, plugins).await?;

    let ports = super::one_shot::HeadlessDeployPorts;
    run_plugin_deploy_inner(&PluginDeployContext {
        folder,
        sealed: sealed.as_ref(),
        managers: &managers,
        sink,
        ports: &ports,
    })
    .await
}

/// Everything the publish half needs once the build is done.
///
/// The split is what the app folded onto at P3: a long-lived process arrives
/// here with a manifest a watcher sealed and in-flight work already drained,
/// and a one-shot arrives with the manifest it just sealed. Both then do the
/// same thing.
pub struct PluginDeployContext<'a> {
    pub folder: &'a Path,
    /// `Option` only because a caller may arrive without one — the app reads
    /// it out of `AppState`, where a folder switch or a failed materialize can
    /// leave the slot empty. Publishing does not tolerate `None`:
    /// [`run_plugin_deploy_inner`] refuses it before any bytes move, and
    /// everything below that refusal takes a `&SealedManifest`.
    pub sealed: Option<&'a SealedManifest>,
    /// The host's plugin managers. The deploy plugin is resolved out of this
    /// rather than passed in, because `[hooks] deploy` is what picks it and
    /// the manager already owns that resolution.
    pub managers: &'a crate::plugins::manager::ManagerCache,
    pub sink: &'a std::sync::Arc<dyn progress::DeploySink>,
    pub ports: &'a dyn crate::build::ports::deploy::DeployPorts,
}

/// The directory the deploy context enumerates — which has to be the one the
/// plugin reads bytes through.
///
/// A plugin gets bytes with `read_site_file`, which resolves under
/// [`MossPaths::current_ptr`], so that is the directory to enumerate — and
/// then to check, because `current` is only the tree this build produced if
/// this build's generation is the one that got promoted. Without the check a
/// build whose promotion was withheld or superseded ships the previous
/// generation while reporting the new file list — green, wrong, and invisible
/// to every layer below this one.
///
/// The app's own body enumerated `staging_dir()` until track P slice P3, and
/// got away with it because a watcher has usually promoted `current` by the
/// time a person clicks Publish. Usually is the whole objection: `staging/` is
/// the mutable directory the NEXT build writes into, so the two are the same
/// tree only by timing, and neither the publish freeze nor any test could say
/// which tree a given click enumerated. It folded onto this.
fn site_dir_for_plugin(mp: &MossPaths, sealed: &SealedManifest) -> Result<PathBuf, String> {
    let promoted = mp
        .current_generation_id()
        .map_err(|e| format!("this build materialized no generation to publish: {e}"))?;
    if promoted != sealed.generation_id() {
        return Err(format!(
            "this build sealed generation {} but the active generation is {promoted} — publishing now \
             would ship one tree and report another",
            sealed.generation_id()
        ));
    }
    Ok(mp.current_ptr())
}

/// Hand a built generation to the folder's deploy plugin, and record what it
/// reported.
pub async fn run_plugin_deploy_inner(
    cx: &PluginDeployContext<'_>,
) -> Result<DeployResult, String> {
    let folder_str = cx.folder.to_string_lossy().to_string();
    let mp = MossPaths::new(cx.folder);

    // The door guard every config-reading door shares
    // (`site_config::ensure_config_current`). Here, not in `with_publish_guard`:
    // the app's `deploy_site` wraps `deploy_site_body` (which calls straight
    // into this function) in that guard, but `moss deploy`'s plugin route
    // (`cli/deploy.rs`) calls `run_plugin_deploy` → this function directly,
    // never through `with_publish_guard` — the CLI is a one-shot process with
    // no live watcher to freeze, so it never needed that guard's single-flight
    // latch. This function is the one place both callers actually converge.
    crate::build::site_config::ensure_config_current(&folder_str)?;

    // The missing-media gate, for every plugin publish there is. It lives here
    // rather than in each caller because "after the build, before any bytes
    // move" is exactly where this body starts: a one-shot arrives having just
    // run `run_pipeline`, and the app arrives after its in-flight drain and
    // `rebuild_if_stale`. Asked any earlier on either route, a half-encoded
    // site reads as a broken one; asked later, the bytes have already left.
    crate::deploy::refuse_publish(&folder_str)?;

    // Before anything else, and before any bytes move: without a sealed
    // manifest there is no generation to check `current` against, and
    // `current` on this path is the PREVIOUS build's tree.
    let sealed = super::one_shot::require_sealed(cx.sealed)?;
    let output_dir = site_dir_for_plugin(&mp, sealed)?;

    let manager = match cx.managers.get_or_create(&folder_str) {
        Ok(m) => m,
        Err(e) => {
            return Ok(deploy_failed(e, crate::infra::app_advisory::t("deploy_err_plugin")))
        }
    };

    // Walks the generation to enumerate what ships.
    cx.sink.stage(
        DeployStage::Syncing,
        0,
        0,
        &crate::infra::app_advisory::t("comparing_with_server"),
    );
    let context = match build_deploy_context(&folder_str, &output_dir) {
        Ok(c) => c,
        Err(e) => {
            return Ok(deploy_failed(e, crate::infra::app_advisory::t("deploy_err_config")))
        }
    };

    // GitHub Pages on a project repo serves the site from a subpath, where
    // moss's absolute paths (`/style.css`, `/article/`) 404.
    if refuses_subpath_pages(
        crate::build::site_config::current_deploy_plugin(&folder_str).as_deref(),
        context.domain.as_deref(),
        get_git_origin(&folder_str).ok().as_deref(),
    ) {
        return Ok(deploy_failed(
            "Cannot deploy to a project repository on GitHub Pages without a custom domain. \
             moss uses absolute paths that break on subpath hosting. Set up a custom domain \
             first, or use moss hosting."
                .to_string(),
            crate::infra::app_advisory::t("deploy_err_custom_domain"),
        ));
    }

    // The bytes leave inside `execute_deploy`, which reports nothing back
    // until it returns.
    cx.sink.stage(
        DeployStage::Uploading,
        0,
        0,
        &crate::infra::app_advisory::t("sending_site"),
    );
    // Claim the publish lease before any byte moves, and on BOTH routes.
    //
    // The app's supervisor restarts a stack it judges un-served, and a
    // terminal publish is invisible to it: the lease it used to consult lived
    // in the app's own memory, so `moss deploy` had nothing to write to and a
    // moss window left open would bounce the stack mid-upload. The lease is a
    // file now, so this one line covers the route that could not reach it.
    onionpress_publish_lease(&folder_str);
    let result = match manager.execute_deploy(&context).await {
        Ok(Some(hook_result)) => {
            if hook_result.success {
                if let Some(deployment) = &hook_result.deployment {
                    // Renewed on landing: the receiver has committed, but its
                    // reachability verification is part of the publish and a
                    // restart during it costs the user the verdict.
                    onionpress_publish_lease(&folder_str);
                    record_landing(cx, sealed, &folder_str, deployment).await;
                }
            }
            DeployResult::Completed { result: hook_result }
        }
        Ok(None) => DeployResult::NoPlugin {
            message: "No deployment plugin configured.\n\nTo deploy, install a deploy plugin or \
                      enable one in .moss/config.toml"
                .to_string(),
        },
        // Catastrophic plugin failure, not a plugin-reported one.
        Err(e) => deploy_failed(e.clone(), e),
    };

    Ok(result)
}

/// Take the machine-scoped publish lease, if and only if this folder publishes
/// to the Tor stack.
///
/// Every other target is somebody else's machine: there is no local stack to
/// leave un-restarted, and a lease taken for a GitHub Pages publish would
/// suppress a recovery the stack it names actually needs.
///
/// Returns nothing and holds nothing: a lease is a file, not a guard, because
/// it must outlive a publish that dies mid-upload and expire on its own
/// afterwards.
fn onionpress_publish_lease(folder_str: &str) {
    use crate::deploy::stack_activity::{unix_secs, StackActivity, ONIONPRESS, PUBLISH_LEASE};
    if crate::build::site_config::current_deploy_plugin(folder_str).as_deref() != Some(ONIONPRESS) {
        return;
    }
    if let Ok(activity) = StackActivity::machine() {
        activity.note_publish(unix_secs(), PUBLISH_LEASE);
    }
}

/// Everything a landed plugin publish owes, once the plugin has reported
/// success WITH a deployment — which is this route's proof that it landed (for
/// OnionPress, after the receiver's `/commit` returned).
///
/// A plugin that claims success without a deployment never reaches here and
/// stays honestly unclassified: there is nothing trustworthy to say went live.
async fn record_landing(
    cx: &PluginDeployContext<'_>,
    sealed: &SealedManifest,
    folder_str: &str,
    deployment: &crate::plugins::types::DeploymentInfo,
) {
    let metadata = if deployment.metadata.is_empty() {
        None
    } else {
        Some(deployment.metadata.clone())
    };
    if let Err(e) = crate::vault::deployment_state::record_publish(
        folder_str,
        crate::vault::deployment_state::PublishOutcome {
            url: deployment.url.clone(),
            deployed_at: deployment.deployed_at.clone(),
            // The plugin's own canonical URL (e.g. an OnionPress onion
            // address), so the next build bakes it into canonical/OG/RSS
            // instead of localhost.
            site_url: Some(deployment.url.clone()),
            dns_target: deployment.dns_target.clone(),
            metadata,
            addresses: deployment.addresses.clone(),
            generation_id: Some(sealed.generation_id().to_string()),
        },
    ) {
        log::warn!("failed to save deployment info to state.toml: {}", e);
    }

    // Keyed by `slot_for`, never the deployment URL: a target that re-mints
    // its URL on every publish would orphan the record it just wrote.
    if let Some(target) = deployment::slot_for(&deployment.method, None) {
        let history = crate::deploy::history::HistoryStore::in_vault(cx.folder);
        crate::deploy::landed::record_landed(cx.folder, sealed, &target, cx.ports, Some(&history)).await;
    }

    // The bytes are live; what is left is redirects/analytics/DNS.
    cx.sink.stage(
        DeployStage::Finalizing,
        0,
        0,
        &crate::infra::app_advisory::t("finalizing"),
    );
    // The documented seam, rather than `domain::orchestrator` directly. The
    // app's own body reached past the port to the orchestrator until track P
    // slice P3 deleted that call; the app-only advisory it carried — a
    // configured domain left without DNS management by an unverified identity
    // — moved into the port's implementation, where both publish routes now
    // reach it. Headless the whole thing is a no-op.
    cx.ports.ensure_domain_ready(folder_str).await;
}

#[cfg(test)]
#[path = "plugin_push_tests.rs"]
mod tests;
