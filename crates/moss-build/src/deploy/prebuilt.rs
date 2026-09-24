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

use crate::build::assets::paths::compute_manifest_generation_id;
use crate::deploy::{progress, PushResult};
use crate::deploy::progress::DeploySink;
use std::sync::Arc;
use crate::identity::Identity;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Walk `dir` and build a moss-seta manifest (`path → "100644:<sha256>"`).
///
/// Paths are forward-slash separated and relative to `dir`. Symlinks within
/// the tree are followed transparently — the upload path uses the file's
/// canonical bytes, so a sym-linked asset uploads as its target. This
/// differs from the moss-format pipeline, which preserves symlinks as
/// `120000:<target>` entries. Static-site outputs rarely contain symlinks;
/// preserving them adds complexity for no observable benefit.
pub fn build_manifest_from_dir(dir: &Path) -> Result<HashMap<String, String>, String> {
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
        // allow:raw_read built output being uploaded — regenerable, dataless is absent (ADR-043)
        let bytes = std::fs::read(path)
            .map_err(|e| format!("read {}: {}", path.display(), e))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = hex::encode(hasher.finalize());
        manifest.insert(rel_str, format!("{}:{}", crate::types::content::MODE_FILE, hash));
        // Hashing is pure local work that happens BEFORE the first request, and
        // it scales with the site. Without a bump per file the stall watchdog
        // wrapping `push_prebuilt` would cancel a big prebuilt tree during a
        // phase where nothing is wrong at all.
        crate::infra::liveness::bump();
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
    sink: &Arc<dyn DeploySink>,
) -> Result<PushResult, String> {
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
            sink,
        )),
        crate::infra::liveness::STALL_TIMEOUT,
        "prebuilt deploy",
    )
    .await
}

async fn push_prebuilt_inner(
    project_folder: &Path,
    prebuilt_dir: &Path,
    site_id: &str,
    identity: &Identity,
    sink: &Arc<dyn DeploySink>,
) -> Result<PushResult, String> {
    // The door guard every config-reading door shares
    // (`site_config::ensure_config_current`) — `push_prebuilt` (the only
    // caller, for both binaries) already takes `with_publish_guard`, but the
    // check lives here, at the same call-order position as the hosted and
    // plugin routes' own "_inner" bodies, so all three read the same way.
    crate::build::site_config::ensure_config_current(&project_folder.to_string_lossy())?;

    sink.stage(
        progress::DeployStage::Preparing,
        0,
        0,
        &crate::infra::app_advisory::t("preparing_publish"),
    );

    // 1. Hash the directory into a manifest.
    let mut manifest = build_manifest_from_dir(prebuilt_dir)?;
    log::info!(
        "deploy(prebuilt): hashed {} files from {}",
        manifest.len(),
        prebuilt_dir.display()
    );

    // Compute the generation_id early so it's available for both the
    // short-circuit (Piece 3) and the commit + save calls below.
    let generation_id = compute_manifest_generation_id(&manifest);

    let env = crate::build::site_config::resolve_environment(&project_folder.to_string_lossy());
    let client = crate::seta::client::MossSetaClient::for_environment(identity, &env);

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

    // 1c. Short-circuit: if the server is already live on our generation,
    //     skip the whole sync/upload/commit cycle. Any Err or Ok(None) from
    //     get_live_generation (404 = old server, network error, site never
    //     deployed) falls through to the normal path for back-compat.
    if let Ok(Some(live_gen)) = client.get_live_generation(site_id).await {
        if live_gen == generation_id {
            log::info!(
                "deploy(prebuilt): generation {} already live on server, skipping deploy",
                live_gen
            );
            // Complete comes from `with_publish_guard` on the way out.
            return Ok(PushResult::Success {
                url: crate::deploy::view_site_url(&folder_str, site_id, env.clone()),
                files_uploaded: 0,
                files_removed: 0,
            });
        }
    }

    // 2. Sync manifest with seta — get back the diff of what to upload/remove.
    sink.stage(
        progress::DeployStage::Syncing,
        0,
        0,
        &crate::infra::app_advisory::t("comparing_with_server"),
    );
    let diff = client
        .sync_manifest(site_id, &manifest, &generation_id)
        .await
        .map_err(|e| e.to_string())?;

    if diff.need.is_empty() && diff.remove.is_empty() {
        // Complete comes from `with_publish_guard` on the way out.
        return Ok(PushResult::Success {
            url: crate::deploy::view_site_url(&folder_str, site_id, env.clone()),
            files_uploaded: 0,
            files_removed: 0,
        });
    }

    // 3. Upload the files seta is missing, through the same window and the same
    //    per-file routing as the interactive publish (crate::deploy::upload).
    //
    //    This loop used to be a private copy that had drifted: 20-wide batches
    //    and NO size routing whatsoever, so a 100 MB video went out as a single
    //    PUT and got a 524 from Cloudflare. Sharing the routing is the point —
    //    a fix in one loop was not a fix.
    //
    //    The one thing that cannot be shared is the digest: manifests built
    //    here are Sha256 (see build_prebuilt_manifest), while deploy.rs seals
    //    xxh3_64. Hence HashAlgo as a parameter. Passing Sha256 here also means
    //    prebuilt deploys now verify integrity at all, which they never did.
    let total = diff.need.len() as u32;
    let uploaded = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let canonical_base = prebuilt_dir
        .canonicalize()
        .map_err(|e| format!("Failed to resolve prebuilt dir: {}", e))?;

    let mut window = crate::deploy::upload::UploadWindow::new();
    // Same self-heal accounting as `push_site_inner_impl`: `commit_sync`
    // below sends `manifest` verbatim as the server's new source of truth,
    // so a file that self-heals during upload needs its manifest entry
    // corrected before that call, and this deploy needs the same cap on how
    // many files may do so before the pattern itself fails the publish.
    let self_heal_cap = crate::deploy::upload::self_heal_cap(diff.need.len());
    let self_heal_corrections: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<String, String>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    for file_path in &diff.need {
        // Size drives window admission, so it must be known before the task
        // starts. A path that cannot be stat'd is admitted as 0 and fails
        // inside the task with the existing, more specific error.
        let file_size = std::fs::metadata(prebuilt_dir.join(file_path))
            .map(|m| m.len())
            .unwrap_or(0);
        let admit_size = window.admission_cost(file_size);
        window.reserve(admit_size).await?;
        let entry_value = manifest.get(file_path).cloned().unwrap_or_default();
        {
            let client = client.clone();
            let site_id = site_id.to_string();
            let prebuilt_dir = prebuilt_dir.to_path_buf();
            let canonical_base = canonical_base.clone();
            let file_path = file_path.clone();
            let sink = Arc::clone(sink);
            let uploaded = uploaded.clone();
            let generation_id = generation_id.clone();
            // Shared per-deploy link estimate; see moss_build::seta::upload_policy.
            let throughput = window.throughput();
            let self_heal_corrections = std::sync::Arc::clone(&self_heal_corrections);

            window.spawn(admit_size, async move {
                let full_path = prebuilt_dir.join(&file_path);
                // Path-traversal guard. Server enforces this too; client-side
                // catch surfaces malformed manifests with a clear error
                // instead of a 400 from the upload PUT.
                let canonical = full_path
                    .canonicalize()
                    .map_err(|e| format!("Failed to resolve {}: {}", file_path, e))?;
                if !canonical.starts_with(&canonical_base) {
                    return Err(format!("Path traversal detected: {}", file_path));
                }
                let (_mode, expected_hash) = crate::types::content::parse_entry(&entry_value);
                let file_size = std::fs::metadata(&canonical)
                    .map_err(|e| format!("Failed to stat {}: {}", file_path, e))?
                    .len();
                let healed_hash = crate::deploy::upload::upload_regular_file(
                    &client,
                    &site_id,
                    &file_path,
                    &canonical,
                    file_size,
                    &generation_id,
                    expected_hash,
                    crate::deploy::upload::HashAlgo::Sha256,
                    &throughput,
                    self_heal_cap,
                    // This path reports FILE-count progress, so it passes no
                    // byte sink — but the stall watchdog needs the byte credit
                    // even when the UI doesn't. Without it a single large file
                    // on a slow link (2000s for 100 MB at 50 KB/s) would look
                    // identical to a wedged socket.
                    Some(&|_n: u64| crate::infra::liveness::bump()),
                )
                .await?;
                if let Some(actual_hash) = healed_hash {
                    self_heal_corrections.lock().unwrap().insert(
                        file_path.clone(),
                        crate::types::content::file_entry(&actual_hash),
                    );
                }
                crate::infra::liveness::bump();
                // follow-up: this prebuilt (CLI build+deploy) path still emits
                // FILE-COUNT progress; the interactive publish (deploy.rs) emits
                // byte-based progress via a ticker (docs/archive/2026-06-11-deploy-
                // upload-progress.md). The frontend falls back to file-count when
                // byte fields are absent, so the hairline still moves here — just
                // by file count. C4b unifies the two loops, when the prebuilt path
                // crosses with the state.toml publish writer it needs.
                let done = uploaded.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                sink.stage(
                    progress::DeployStage::Uploading,
                    done,
                    total,
                    &crate::build::progress::format_progress_message(
                        &crate::infra::app_advisory::fmt(
                            "uploading",
                            &[("name", &file_path)],
                        ),
                        done,
                        total,
                    ),
                );
                Ok::<(), String>(())
            });
        }
    }
    window.drain().await?;

    // Fold in any self-heals discovered during upload — see the comment
    // where `self_heal_corrections` is created. `generation_id` was already
    // computed from the pre-correction manifest above and stays fixed: it
    // names the server-side directory this upload actually went into, not a
    // hash of the manifest's final content.
    {
        let corrections = self_heal_corrections.lock().unwrap();
        if !corrections.is_empty() {
            log::info!(
                "deploy(prebuilt): correcting {} manifest {} to actually-shipped hashes before commit",
                corrections.len(),
                if corrections.len() == 1 { "entry" } else { "entries" }
            );
        }
        for (path, entry) in corrections.iter() {
            manifest.insert(path.clone(), entry.clone());
        }
    }

    // 4. Commit — activates the new manifest server-side.
    sink.stage(
        progress::DeployStage::Committing,
        0,
        0,
        &crate::infra::app_advisory::t("making_changes_live"),
    );
    // generation_id was computed above after building the manifest.
    let commit_result = client
        .commit_sync(site_id, &manifest, &generation_id)
        .await
        .map_err(|e| e.to_string())?;

    // Skew detection: the server echoes its now-live generation_id.
    // If it differs from what we just published, two deploys may have raced
    // (oscillation / out-of-order commit). Log for diagnosis only.
    // TODO: expose as a UI advisory (PushResult advisory slot) once bindings
    //       churn is acceptable.
    // Unlike `push.rs`'s (task 3b): no `PublishVerdict` burst — known gap,
    // see plan's Step 3 "known scoped-out gap" note.
    if let Some(server_gen) = commit_result.generation_id.as_deref() {
        if server_gen != generation_id {
            log::warn!(
                "deploy: generation skew — published {} but server is live on {} (possible oscillation)",
                generation_id, server_gen
            );
        }
    }

    // 5. Save deployment info to state.toml so the user-visible "last published"
    //    URL stays accurate and re-deploys are idempotent.
    let folder_path_str = project_folder.to_string_lossy().to_string();
    let deployed_at = chrono::DateTime::from_timestamp(commit_result.timestamp as i64, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339();
    // The record a later `moss deploy` compares the live site against, from
    // either route: without it, a folder that alternates the two routes reads
    // its own prebuilt publish as someone else's and is refused.
    if let Some(target) = crate::config::deployment::slot_for("moss", Some(site_id)) {
        crate::deploy::landed::record_prebuilt_landed(project_folder, &generation_id, &target, &manifest);
    }
    if let Err(e) = crate::vault::deployment_state::record_publish(
        &folder_path_str,
        crate::vault::deployment_state::PublishOutcome {
            url: commit_result.url.clone(),
            deployed_at,
            generation_id: Some(generation_id.clone()),
            // moss-hosted prebuilt deploy — canonical URL derives from site_id.
            ..Default::default()
        },
    ) {
        log::warn!("Failed to save deployment info: {}", e);
    }

    // Complete comes from `with_publish_guard`, which wraps this body — one
    // exit for every publish. The `total, total` counts were never read: both
    // listeners stop reading fields once the stage is terminal.

    // Use the env-derived URL rather than the server-echoed one so staging
    // deploys surface the correct staging URL (not the prod-shaped echo).
    // Mirrors the already-fixed short-circuit paths at :207/:236. Like those
    // paths, the custom domain wins once DNS is verified.
    Ok(PushResult::Success {
        url: crate::deploy::view_site_url(&folder_path_str, site_id, env),
        files_uploaded: commit_result.files_updated,
        files_removed: commit_result.files_removed,
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
    sink: &Arc<dyn DeploySink>,
) -> Result<PushResult, String> {
    let Some((site_id, identity)) =
        crate::deploy::resolve_publish_inputs(folder, requested_site_id, overwrite_newer, sink).await?
    else {
        return Ok(PushResult::NeedsSetup);
    };

    push_prebuilt(folder, dir, &site_id, &identity, sink).await
}
