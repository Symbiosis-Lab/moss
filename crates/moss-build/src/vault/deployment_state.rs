//! Writing `.moss/state.toml`'s `[deployment]` — the machine-managed record of
//! what a publish did.
//!
//! Its sibling [`super::config`] writes `config.toml`, the file a person
//! hand-edits; this one is written only by moss and read back only by moss.
//! They share the same write frame (`load_managed_toml` / `write_managed_toml`)
//! because a rewrite that re-emits a whole TOML document is the bug ADR-059
//! was written about, and that lesson does not stop being true because the
//! file is machine-owned.
//!
//! Crossed here 2026-09-07 ahead of track C4c, which gives `moss-cli` a
//! `deploy` verb: a terminal publish has to record what it did the way the app
//! does, or the two binaries disagree about what a folder's last publish was.
//! Until that verb lands the only consumer is the app, through the re-exports
//! in `domain::config`.

use crate::config::deployment::{
    DeployAddress, DeploymentState, DnsTarget, DomainDeploymentConfig, DomainObservation,
};
use crate::vault::config::{load_managed_toml, write_managed_toml, ManagedToml};
use std::path::Path;

/// The ONE write frame for `.moss/state.toml [deployment]`: load, parse the
/// standing persisted shape out of the loaded document (one read, no re-fetch
/// race), mutate, serialize back over the original bytes. Both state writers
/// — [`save_domain_config`] (absorb) and [`update_domain_observation`] — go
/// through here, so "state.toml has one write frame" is structural rather
/// than conventional.
fn update_deployment_state<F>(project_path: &str, mutate: F) -> Result<(), String>
where
    F: FnOnce(&mut DeploymentState),
{
    let state_path = Path::new(project_path).join(".moss").join("state.toml");
    let ManagedToml { original, mut root } = load_managed_toml(&state_path)?;
    let mut state = DeploymentState::from_toml(root.get("deployment"))?;
    mutate(&mut state);
    let deployment_value = toml::Value::try_from(&state)
        .map_err(|e| format!("Failed to serialize deployment config: {}", e))?;
    root.insert("deployment".to_string(), deployment_value);
    write_managed_toml(&state_path, &original, &root)
}

/// Update `[deployment].observed` in state.toml — the ONE writer for the
/// [`DomainObservation`] record. `mutate` receives the current record
/// (default-fresh when none exists) and returns the record to store, or
/// `None` to delete it. Every caller replaces the record as a unit; there is
/// no field-at-a-time write path, which is what made the old scattered
/// `dns_*` fields able to disagree.
pub fn update_domain_observation<F>(project_path: &str, mutate: F) -> Result<(), String>
where
    F: FnOnce(DomainObservation) -> Option<DomainObservation>,
{
    update_deployment_state(project_path, |state| {
        state.observed = mutate(state.observed.take().unwrap_or_default());
    })
}

/// Save the flat deployment view back to .moss/state.toml.
///
/// The flat view is split, not stored: project-scoped fields overwrite in
/// place, per-target fields land in the ACTIVE target's
/// `[deployment.targets.<slot>]` record, and every other target's record is
/// carried through untouched — so a moss deploy structurally cannot reach
/// what moss knows about an onion, and vice versa. The derived
/// `deploy_method` on the view is dropped here: `[hooks] deploy` in
/// config.toml is the selector's only home.
pub fn save_domain_config(
    project_path: &str,
    config: &DomainDeploymentConfig,
) -> Result<(), String> {
    // Absorb into the standing persisted shape so sibling targets' records
    // survive the round-trip — the flat view only ever carries the active
    // target's.
    update_deployment_state(project_path, |state| state.absorb(config))
}

/// What one successful publish reports back.
///
/// Every field is written to the active target's deployment record, so this
/// struct is the answer to "what does a publish know that the record keeps".
/// Construct it with struct-update syntax — `PublishOutcome { url,
/// deployed_at, ..Default::default() }` — and set only what the path
/// actually produces. `last_verified_generation` is deliberately absent: it
/// records a later check, not this publish, and is preserved untouched.
#[derive(Debug, Clone, Default)]
pub struct PublishOutcome {
    /// Where the site went live. Required.
    pub url: String,
    /// RFC 3339. Required.
    pub deployed_at: String,
    /// The plugin's own canonical url (e.g. an OnionPress onion address), or
    /// `None` for moss hosting, whose URL derives from site_id at resolution
    /// time (persisting it would freeze the environment suffix).
    pub site_url: Option<String>,
    pub dns_target: Option<DnsTarget>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
    /// Every way to reach the published site — see
    /// [`DeployAddress`].
    pub addresses: Vec<DeployAddress>,
    /// The generation this publish shipped. `None` from callers with no
    /// generation context, which must NOT erase a previously-persisted id.
    pub generation_id: Option<String>,
}

/// Record what one publish did, preserving every field it does not report.
///
/// This is the single writer all three publish paths reach, which is what makes
/// it the one place that sees every successful publish regardless of deploy
/// target. A terminal publish records here and decorates nothing; the app also
/// stamps the folder's Finder icon, which is objc2/AppKit and cannot cross.
///
/// **The stamp is not proved by this function's name.** It used to be: all
/// three app paths spelled `domain::config::save_deployment_to_config`, the
/// wrap that stamps, and reaching this writer directly was the mistake the two
/// names existed to make hard. All three now reach it directly — the prebuilt
/// path since track C4c, the moss-hosted body since C4e, the deploy-plugin
/// body since track P slice P3 — because the body each lives in crossed to
/// this crate, and the wrap was deleted with the last of them. So the app
/// stamps at each of its three call sites instead, and what holds that
/// together is
/// `publish_record_ordering_test::every_app_publish_path_stamps_the_folder`.
/// Add an app-side publish path and that scan is what you must extend.
pub fn record_publish(
    project_path: &str,
    outcome: PublishOutcome,
) -> Result<(), String> {
    let PublishOutcome {
        url,
        deployed_at,
        site_url,
        dns_target,
        metadata,
        addresses,
        generation_id,
    } = outcome;

    // Get existing config to preserve other fields
    let mut config = crate::build::site_config::get_domain_config(project_path)?;

    // Update deployment fields — all per-target, so `save_domain_config`
    // files them under the active target's record and no other target's
    // state is reachable from here.
    config.last_deployment_url = Some(url);
    config.last_deployment_at = Some(deployed_at.clone());
    config.metadata = metadata;
    config.addresses = addresses;
    config.site_url = site_url;
    // Non-clobbering: see PublishOutcome::generation_id.
    if let Some(g) = generation_id {
        config.last_deployed_generation_id = Some(g);
    }

    // Save updated config
    save_domain_config(project_path, &config)?;

    // A deploy that reports DNS targets is refreshing the domain observation,
    // not the target record — the records describe the domain, which is
    // project-scoped (see `DomainObservation`).
    if let Some(target) = dns_target {
        update_domain_observation(project_path, |mut o| {
            o.dns_target = Some(target);
            o.checked_at = deployed_at;
            Some(o)
        })?;
    }

    Ok(())
}

/// Record that the DNS-verification flow just found the records serving —
/// the one mutation both verification paths (the command and the
/// orchestrator) make, stamped with now.
pub fn record_dns_verified(project_path: &str) -> Result<(), String> {
    update_domain_observation(project_path, |mut o| {
        o.dns_configured = true;
        o.checked_at = chrono::Utc::now().to_rfc3339();
        Some(o)
    })
}

/// Record seta's newest CDN word for the domain, so the next launch can seed
/// the pill's resting state without a network round trip. Does not touch
/// `checked_at`: that stamp belongs to the connect verdict, which the probe
/// refreshes on the same beat the CDN is read on.
pub fn record_cdn_status(project_path: &str, status: &str) -> Result<(), String> {
    update_domain_observation(project_path, |mut o| {
        o.cdn_status = Some(status.to_string());
        Some(o)
    })
}

/// Clear site_id, triggering the first-publish modal on next deploy. Called
/// when the server reports the site no longer exists. With no `[hooks]
/// deploy` and no site_id, the derived `deploy_method` reads back as `None`
/// — there is no stored copy left to clear.
pub fn clear_site_id(project_path: &str) -> Result<(), String> {
    let mut config = crate::build::site_config::get_domain_config(project_path)?;
    config.site_id = None;
    // The per-target fields on this view belong to the moss:<old-id> record,
    // which stays on disk under its own slot; with site_id gone the view
    // names no target, so hand `save_domain_config` nothing to file.
    config.site_url = None;
    config.last_deployment_url = None;
    config.last_deployment_at = None;
    config.metadata = None;
    config.addresses = Vec::new();
    config.last_deployed_generation_id = None;
    config.last_verified_generation = None;
    save_domain_config(project_path, &config)
}

/// Persist a plugin-supplied canonical `site_url` into `[deployment]`.
///
/// Used by the OnionPress name flow: registering the onionname yields the
/// `<addr>.onion` URL, which must land in state.toml BEFORE the pre-deploy
/// rebuild so the first published generation bakes the onion canonical/OG,
/// not localhost. The deploy result re-persists it as a backstop.
pub fn set_site_url(project_path: &str, url: Option<String>) -> Result<(), String> {
    let mut config = crate::build::site_config::get_domain_config(project_path)?;
    config.site_url = url;
    save_domain_config(project_path, &config)
}

/// Ask seta for a domain's CDN status and persist a recognized word into
/// `[deployment].observed.cdn_status` — the one pairing `get_domain_cdn`
/// (desktop) and `moss domain link` (CLI) both need, moved here so neither
/// call site re-types it (2026-09-15).
///
/// Best-effort, full stop: returns `None` — after a `log::warn!` — on either
/// the seta call failing or the persist failing, and never distinguishes the
/// two to the caller. A caller that must turn a *read* failure into a
/// user-visible error (`get_domain_cdn`'s `Result` surface for the frontend)
/// makes its own `client.get_domain_cdn` call for that and does not use this
/// helper for it; this one is for callers — `moss domain link` today — that
/// only want the observation recorded if it's cheap, and must never fail on
/// its account.
pub async fn observe_and_record_cdn(
    client: &crate::seta::client::MossSetaClient,
    host: &str,
    config_path: &str,
) -> Option<crate::seta::cdn::DomainCdn> {
    let cdn = match client.get_domain_cdn(host).await {
        Ok(cdn) => cdn,
        Err(e) => {
            log::warn!(target: "domain", "failed to read cdn status for {}: {}", host, e);
            return None;
        }
    };
    if let Some(word) = cdn.cdn_status.wire_word() {
        if let Err(e) = record_cdn_status(config_path, word) {
            log::warn!(target: "domain", "failed to persist cdn status: {}", e);
            return None;
        }
    }
    Some(cdn)
}

/// Persist the claimed OnionPress name into `[deployment]`.
///
/// Called after a successful `onionpress_name_register`, alongside
/// `set_site_url`. Never called on skip, so a skipped claim simply leaves
/// this field unset.
pub fn set_onion_name(project_path: &str, name: Option<String>) -> Result<(), String> {
    let mut config = crate::build::site_config::get_domain_config(project_path)?;
    config.onion_name = name;
    save_domain_config(project_path, &config)
}
