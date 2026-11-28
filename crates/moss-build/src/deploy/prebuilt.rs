//! Pre-built static-site deploy path.
//!
//! `moss deploy --prebuilt <dir>` (or `[deployment].prebuilt_output` in
//! `.moss/state.toml`) routes through here instead of through moss's
//! markdown build pipeline. The directory is walked, hashed, and uploaded
//! to seta using the same sync → upload → commit contract as a moss-format
//! deploy. Use cases: Quire, Hugo, Jekyll, Astro, or any other SSG whose
//! output is a self-contained tree of HTML/CSS/JS/asset files.
//!
//! What this path intentionally does NOT do (vs. `push_site_inner`):
//! - No redirect-stub generation (moss tracks renames via its own article-map;
//!   external SSGs handle redirects themselves)
//! - No article-map snapshot (no moss build → no article-map), so the
//!   publish record it writes names no pages — see `landed::record_prebuilt_landed`
//! - No subscriber/analytics post-deploy sync (no moss-managed channels)
//! - No domain orchestrator (callers wire this up if they want a custom
//!   domain — orthogonal to the prebuilt question)
//!
//! Everything that IS still done: identity load, manifest hash, sync diff,
//! HTTP/2 parallel upload (20 concurrent), commit, save deployment URL, and
//! the publish record `moss deploy`'s stale-copy check reads.

use crate::deploy::{progress, PushResult};
use crate::deploy::progress::DeploySink;
use std::sync::Arc;
use crate::identity::Identity;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Walk `dir` and build a moss-seta manifest (`path → "100644:<xxh3_64>"`).
///
/// Paths are forward-slash separated and relative to `dir`. Symlinks within
/// the tree are followed transparently — the upload path uses the file's
/// canonical bytes, so a sym-linked asset uploads as its target. This
/// differs from the moss-format pipeline, which preserves symlinks as
/// `120000:<target>` entries. Static-site outputs rarely contain symlinks;
/// preserving them adds complexity for no observable benefit.
pub fn build_manifest_from_dir(dir: &Path) -> Result<HashMap<String, String>, String> {
    let _phase = crate::build::phase::PhaseTrace::start("manifest");
    if !dir.exists() {
        return Err(format!(
            "prebuilt directory does not exist: {}",
            dir.display()
        ));
    }
    if !dir.is_dir() {
        return Err(format!(
            "prebuilt path is not a directory: {}",
            dir.display()
        ));
    }

    let mut manifest = HashMap::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(true) {
        let entry = entry.map_err(|e| format!("walk error: {}", e))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(dir)
            .map_err(|e| format!("strip_prefix error: {}", e))?;
        // Forward-slash-only on the wire; Windows-style backslashes break
        // server-side path joins.
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        // A folder another tool built names no reader, so nothing in it is sent as members-only.
        if crate::build::emit::members::gated_address(&rel_str).is_some() {
            continue;
        }
        // The same streaming xxh3_64 the normal build seals into its manifest,
        // so a prebuilt deploy and a normal publish of identical bytes agree
        // on every entry and on the generation id; the server compares entries
        // as opaque strings. Hashing is pure local work that happens BEFORE
        // the first request and scales with the site, so each buffer bumps the
        // stall clock: without it the watchdog wrapping `push_prebuilt` would
        // cancel a big prebuilt tree during a phase where nothing is wrong.
        let hash = crate::build::assets::paths::compute_binary_hash_file_with_heartbeat(
            path,
            &|| crate::infra::liveness::bump(),
        )?;
        manifest.insert(rel_str, crate::types::content::file_entry(&hash));
    }

    if manifest.is_empty() {
        return Err(format!(
            "prebuilt directory contains no files: {}",
            dir.display()
        ));
    }

    Ok(manifest)
}

/// Resolve the effective prebuilt directory for a deploy.
///
/// Priority order (highest first):
/// 1. `cli_arg` — `--prebuilt=<path>` on the command line
/// 2. `config.prebuilt_output` — `[deployment].prebuilt_output` in state.toml
/// 3. `None` — moss runs its normal markdown build
///
/// Relative paths resolve against `project_folder`.
pub fn resolve_prebuilt_dir(
    project_folder: &Path,
    config: &crate::config::deployment::DomainDeploymentConfig,
    cli_arg: Option<&str>,
) -> Option<PathBuf> {
    let raw = cli_arg.or(config.prebuilt_output.as_deref())?;
    let p = Path::new(raw);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_folder.join(p)
    })
}

/// Push a pre-built static-site directory to seta.
///
/// Caller is responsible for:
/// - Loading the project's `Identity` (`.moss/identity/secret-key`)
/// - Ensuring `site_id` is configured (this fn returns `NeedsSetup` if not)
/// - Confirming the site is registered with seta (call `register_site`
///   before the first deploy of a new site_id)
///
/// Single-flight: this path runs its OWN full upload loop, so it takes the same
/// latch `push_site` takes rather than a private one. `moss deploy --prebuilt`
/// and a Publish from the window are the same act to the user and to the
/// network; whichever arrives second is rejected, not queued. The latch is an
/// in-process atomic, so it does NOT cover two concurrent `moss deploy` OS
/// processes — serializing those would take a filesystem lock, which we do not
/// take. Everything routed through one running moss IS covered.
pub async fn push_prebuilt(
    project_folder: &Path,
    prebuilt_dir: &Path,
    site_id: &str,
    identity: &Identity,
    accept_removals: bool,
    sink: &Arc<dyn DeploySink>,
) -> Result<PushResult, crate::deploy::PublishError> {
    super::profile::record(project_folder, "prebuilt", false, async {
    // Under the same stall watchdog as the interactive publish. This path had
    // NO outer bound at all, so `moss deploy --prebuilt` against a wedged
    // socket hung forever with no error; it now fails after
    // `activity::STALL_TIMEOUT` of nothing happening, while a slow-but-moving
    // upload runs as long as the link needs (I1).
    crate::infra::liveness::bounded_by_stall(
        crate::deploy::freeze::with_publish_guard(sink, push_prebuilt_inner(
            project_folder,
            prebuilt_dir,
            site_id,
            identity,
            accept_removals,
            sink,
        )),
        crate::infra::liveness::STALL_TIMEOUT,
        "prebuilt deploy",
    )
    .await
    }).await
}

/// The last publish record an upload is measured against, and the public
/// addresses it served that `manifest` no longer holds. Both the upload's
/// removal gate and the dry run read this, so they cannot disagree.
pub(crate) fn removed_against_last_publish(
    project_folder: &Path,
    site_id: &str,
    manifest: &HashMap<String, String>,
) -> Option<(crate::build::manifest::change_set::PublishedSnapshot, Vec<String>)> {
    let mp = crate::moss_paths::MossPaths::new(project_folder);
    let target = crate::config::deployment::slot_for("moss", Some(site_id));
    let baseline =
        crate::build::manifest::published_record::load_address_baseline(&mp, target.as_deref())?;
    let gone = crate::build::manifest::change_set::removed_public_paths(&baseline, manifest);
    Some((baseline, gone))
}

/// Refuse an upload that stops serving an address the last publish served.
/// Runs before any request.
fn refuse_removed_addresses(
    project_folder: &Path,
    site_id: &str,
    manifest: &HashMap<String, String>,
) -> Result<(), String> {
    match removed_against_last_publish(project_folder, site_id, manifest) {
        Some((_, gone)) if !gone.is_empty() => Err(crate::deploy::removal_gate::prebuilt_refusal(&gone)),
        _ => Ok(()),
    }
}

async fn push_prebuilt_inner(
    project_folder: &Path,
    prebuilt_dir: &Path,
    site_id: &str,
    identity: &Identity,
    accept_removals: bool,
    sink: &Arc<dyn DeploySink>,
) -> Result<PushResult, crate::deploy::PublishError> {
    // The door guard every publish route shares
    // (`site_config::publish_door`) — `push_prebuilt` (the only
    // caller, for both binaries) already takes `with_publish_guard`, but the
    // check lives here, at the same call-order position as the hosted and
    // plugin routes' own "_inner" bodies, so all three read the same way.
    // moss rendered no page of this directory, so none of it is restricted.
    crate::build::site_config::publish_door(project_folder, false)?;

    sink.stage(
        progress::DeployStage::Preparing,
        0,
        0,
        &crate::infra::app_advisory::t("preparing_publish"),
    );

    crate::build::features::comment::link_block::refuse_prebuilt(project_folder, prebuilt_dir)?;

    // 1. Hash the directory into a manifest.
    let mut manifest = build_manifest_from_dir(prebuilt_dir)?;
    super::profile::count("manifest_files", manifest.len() as u64);
    log::info!(
        "deploy(prebuilt): hashed {} files from {}",
        manifest.len(),
        prebuilt_dir.display()
    );

    if !accept_removals {
        let _phase = crate::build::phase::PhaseTrace::start("local_checks");
        super::profile::gate("removals", refuse_removed_addresses(project_folder, site_id, &manifest))?;
    }

    let env = crate::build::site_config::resolve_environment(&project_folder.to_string_lossy());
    let client = crate::deploy::publish_client(project_folder, identity, &env).await;

    let folder_str = project_folder.to_string_lossy().to_string();

    // Registering used to happen here, and could not: a first prebuilt publish
    // reached this block with `last_deployment_url` still empty — it is not
    // written until the upload commits — so it registered a site the caller had
    // registered seconds earlier, and seta's "pubkey must be fresh" check runs
    // before its idempotent owner-match, which answers a legitimate re-register
    // with a 400. C4g gave registration one owner,
    // `crate::deploy::register_moss_host_site`, which every caller of this
    // function reaches through `resolve_publish_inputs`. One of the block's
    // three refusals was the artifact and went with it; the other two — an
    // unverified pubkey, a name already taken — moved to that owner, so every
    // route has them now instead of this one.

    sink.stage(
        progress::DeployStage::Syncing,
        0,
        0,
        &crate::infra::app_advisory::t("comparing_with_server"),
    );
    let transfer = super::transfer::Transfer {
        client: &client,
        site_id,
        dir: prebuilt_dir,
        sink,
        allow_removal: super::mass_remove::removal_accepted(accept_removals),
    };
    let opened = transfer.open(&manifest).await.map_err(super::profile::profiled_refusal)?;
    if opened.state == crate::seta::deploys::DeployState::Live && opened.files_updated.is_none() {
        log::info!("deploy(prebuilt): generation {} already live on server, skipping deploy", opened.deploy);
        // Complete comes from `with_publish_guard` on the way out.
        return Ok(PushResult::Success {
            url: crate::deploy::view_site_url(&folder_str, site_id, env.clone()),
            files_uploaded: 0,
            files_removed: 0,
        });
    }
    let total = opened.needed.len() as u32;
    let commit_result = transfer.finish(&mut manifest, opened).await?;
    let generation_id = commit_result.deploy.clone();
    let _after_publish = crate::build::phase::PhaseTrace::start("after_publish");

    // 5. Save deployment info to state.toml so the user-visible "last published"
    //    URL stays accurate and re-deploys are idempotent.
    let folder_path_str = project_folder.to_string_lossy().to_string();
    let deployed_at = commit_result.timestamp.and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    // The record a later `moss deploy` compares the live site against, from
    // either route: without it, a folder that alternates the two routes reads
    // its own prebuilt publish as someone else's and is refused.
    if let Some(target) = crate::config::deployment::slot_for("moss", Some(site_id)) {
        crate::deploy::landed::record_prebuilt_landed(project_folder, &generation_id, &target, &manifest);
    }
    let hosted = crate::deploy::hosted_address(&env, site_id, commit_result.url.as_deref().unwrap_or_default());
    if let Err(e) = crate::vault::deployment_state::record_publish(
        &folder_path_str,
        crate::vault::deployment_state::PublishOutcome {
            url: hosted.clone().unwrap_or_else(|| env.assumed_site_url(site_id)),
            deployed_at,
            generation_id: Some(generation_id.clone()),
            site_url: hosted,
            ..Default::default()
        },
    ) {
        log::warn!("Failed to save deployment info: {}", e);
    }

    // Complete comes from `with_publish_guard`, which wraps this body — one
    // exit for every publish. The `total, total` counts were never read: both
    // listeners stop reading fields once the stage is terminal.

    // Like the short-circuit paths, the custom domain wins once DNS is verified.
    Ok(PushResult::Success {
        url: crate::deploy::view_site_url(&folder_path_str, site_id, env),
        files_uploaded: total,
        files_removed: commit_result.files_removed.unwrap_or(0),
    })
}

#[cfg(test)]
#[path = "prebuilt_tests.rs"]
mod tests;

/// One prebuilt publish, from a resolved directory to a result — the whole of
/// what `moss deploy --prebuilt` does, shared by both binaries.
///
/// The app and the CLI differ in exactly one value, the [`DeploySink`], so
/// they call this rather than each assembling the same four steps. Before
/// track C4c the app assembled them inline in `start_deploy` and the open
/// binary could not publish at all; the copy that would have been written for
/// it is the twin this exists to prevent.
///
/// `dir` is already resolved, by [`resolve_prebuilt_dir`], because both
/// callers must resolve before they can call: the app to decide whether to run
/// moss's own build instead, the CLI to word its own refusal. Resolving again
/// in here would re-read the same config file to reach an answer the caller
/// already holds.
pub async fn run_prebuilt_deploy(
    folder: &Path,
    dir: &Path,
    requested_site_id: Option<&str>,
    overwrite_newer: bool,
    accept_removals: bool,
    sink: &Arc<dyn DeploySink>,
) -> Result<PushResult, crate::deploy::PublishError> {
    super::profile::record(folder, "prebuilt", false, async {
    let Some((site_id, identity)) =
        crate::deploy::resolve_publish_inputs(folder, requested_site_id, overwrite_newer, sink).await?
    else {
        return Ok(PushResult::NeedsSetup);
    };

    push_prebuilt(folder, dir, &site_id, &identity, accept_removals, sink).await
    }).await
}
