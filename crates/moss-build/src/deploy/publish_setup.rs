//! The publish setup gate: is the target a vault publishes through ready, and
//! if not, what credential is missing?
//!
//! The gate resolves setup for a **(plugin, capability)** pair, not for the
//! deploy target — a channel declares the same block a deploy target does,
//! so the answer must not be reachable only
//! from the deploy path. Publish is the one caller today: it resolves the
//! active target and asks about [`Capability::Deploy`].
//!
//! Everything here answers from the manifest and the credential store with no
//! plugin code running, which is what makes the common case — already set up —
//! free on every Publish click, and is also why it lives below both binaries:
//! a headless publish never runs the frontend modal, and a plugin handed no
//! token fails deep inside its own upload with whatever the remote service
//! says. The plugin's own probe (`check_setup`) costs an engine dispatch and
//! needs a person to answer it, so it stays app-side with the modal, in
//! the desktop app's `plugins::commands::publish_setup`.
//!
//! It sits under `deploy` because that is what it gates. What it reads is a
//! contribution's `setup` block, but what it depends on is plugin discovery,
//! the registry's display names and the credential store — not manifest
//! shapes — and the tree's other publish refusal, [`crate::deploy::refuse_publish`],
//! is the one it is asked beside: `publish_gate_invariant_test` names both at
//! the same call site.

use crate::build::site_config::current_deploy_plugin;
use crate::identity::keystore::Scope;
use crate::identity::secrets::SecretStore;
use crate::plugins::contributions::{Field, Need, SetupContribution};
use crate::plugins::setup::{SetupBlocker, SetupForm};
use crate::plugins::types::{Capability, PluginManifest};

/// One credential moss is about to ask the user for.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CredentialPrompt {
    /// The store key — what the write command needs back.
    pub key: String,
    /// What to call the field: "Pinata API token", not "pinata_jwt".
    pub label: String,
    /// Where the user gets one. A prompt with no way to obtain the credential
    /// is a dead end, so the modal renders this as a link when present.
    pub help_url: Option<String>,
    /// Whether moss already holds a value under [`key`](Self::key).
    ///
    /// Carried on the prompt rather than kept as a second list of the same
    /// credentials, because the two questions differ only by this bit:
    /// supplying a missing credential asks for `!stored`, correcting a
    /// rejected one asks for all of them, and the publish gate refuses on
    /// `any(!stored)`. Every reader derives its own subset from the one list.
    pub stored: bool,
}

impl CredentialPrompt {
    /// A prompt for a declared `secret` field, told whether moss already holds
    /// a value for it.
    ///
    /// `stored` is passed in rather than read here: the setup gate takes it
    /// from an injected answer so its tests need no real credential store.
    pub fn for_field(field: &crate::plugins::contributions::Field, stored: bool) -> Self {
        Self {
            key: field.key.clone(),
            label: field.label.clone(),
            help_url: field.help_url.clone(),
            stored,
        }
    }
}

/// What a plugin's contribution still needs before moss can use it.
///
/// The name is publish's, because publish is the only caller; the shape is
/// generic and the type is what the modal reads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PublishSetupNeeds {
    /// The plugin the credentials belong to, for the write command.
    pub plugin: String,
    /// Its human name, for the modal's title.
    pub plugin_name: String,
    /// Every credential this contribution declares, each carrying whether moss
    /// already holds it. All stored — or none declared — means ready.
    pub credentials: Vec<CredentialPrompt>,
    /// The probe step has something to ask: the plugin declared `check_setup`,
    /// or manifest `needs` the host must evaluate before publishing. The probe
    /// modal (its own step) runs [`run_publish_setup_check`], which
    /// short-circuits failed needs without spawning the plugin.
    pub wants_check: bool,
}

/// [`deploy_setup_needs`] against the real credential store — the answer the
/// app's `check_publish_setup` command returns to the modal, and the one
/// [`refuse_publish`] refuses on.
pub fn needs_from_store(folder_path: &str) -> Result<Option<PublishSetupNeeds>, String> {
    let store = SecretStore::require("the publish setup gate")?;
    Ok(deploy_setup_needs(folder_path, &|plugin, key| {
        store.get(&Scope::Plugin(plugin.to_string()), key).ok().flatten().is_some()
    }))
}

/// The publish gate's own question: what does the target this vault publishes
/// through still need?
///
/// Resolving the target is the publish-specific half, and it is asked of the
/// one function that owns that policy — the same one the Host row reads.
/// Resolving it here a second way is what made this gate ask a moss-hosted
/// user to sign in to GitHub: `resolve_deploy_plugin` falls back to the sole
/// installed deploy plugin, but choosing moss hosting CLEARS `[hooks] deploy`,
/// so the fallback fired for a publish that never touches the plugin.
fn deploy_setup_needs(
    folder_path: &str,
    has_secret: &dyn Fn(&str, &str) -> bool,
) -> Option<PublishSetupNeeds> {
    let target = current_deploy_plugin(folder_path)?;
    setup_needs(folder_path, &target, &Capability::Deploy, has_secret)
}

/// What one plugin's contribution still needs, given any answer to "is this
/// credential stored?".
///
/// The capability is what selects the setup block, so the same gate serves a
/// deploy target and a channel: the manifest says where the block sits, and
/// this asks the manifest rather than a fixed path through it.
///
/// The store is app-global, so a test that used the real one would read the
/// developer's own credentials and pass or fail depending on them. Everything
/// worth testing is above the store anyway: which of a plugin's declared
/// credentials this user's settings select, and what the modal is told to ask
/// for.
fn setup_needs(
    folder_path: &str,
    plugin_id: &str,
    capability: &Capability,
    has_secret: &dyn Fn(&str, &str) -> bool,
) -> Option<PublishSetupNeeds> {
    let (manifest, setup) = declared_setup(folder_path, plugin_id, capability)?;

    // Defaults include declared field defaults, so a `when` on a field the
    // user never touched still resolves the way the settings form shows it.
    let config = crate::plugins::discovery::get_plugin_config_with_defaults(
        folder_path,
        plugin_id,
        &manifest.config_defaults(),
    );

    let credentials = setup
        .applicable_secrets(&config)
        .into_iter()
        .map(|field| CredentialPrompt::for_field(field, has_secret(plugin_id, &field.key)))
        .collect();

    Some(PublishSetupNeeds {
        // The same words the row the user picked it from uses — the modal must
        // not call the contribution something else.
        plugin_name: crate::plugins::registry::contribution_display_name(&manifest, capability),
        plugin: plugin_id.to_string(),
        credentials,
        wants_check: setup.check || !setup.needs.is_empty(),
    })
}

/// The manifest and the `setup` block it declares for `capability` — the one
/// walk from a plugin id to what it declared, shared by the credential half
/// and by [`deploy_target_setup`].
///
/// A read, so the read-only discovery: this runs on every Publish click, and
/// installing the bundled set from a gate would be a write nobody asked for.
fn declared_setup(
    folder_path: &str,
    plugin_id: &str,
    capability: &Capability,
) -> Option<(PluginManifest, SetupContribution)> {
    let plugins = crate::plugins::discovery::discover_installed_plugins(folder_path).ok()?;
    let plugin = plugins.into_iter().find(|p| p.manifest.name == plugin_id)?;
    let setup = plugin.manifest.setup_for(capability)?.clone();
    Some((plugin.manifest, setup))
}

/// What the chosen deploy target declares that the app's probe step needs: its
/// id, the preconditions the host evaluates before spawning `check_setup`, and
/// the settings a submitted form routes home by.
///
/// The app asks this instead of resolving the target itself. It used to walk
/// `current_deploy_plugin` → discovery → find-by-name → `setup_for` a second
/// time, which is one gate with two resolutions: the half that refuses on a
/// missing credential and the half that probes could name different plugins
/// after any change to how a target is chosen.
pub struct DeployTargetSetup {
    /// The plugin the credentials and the config edit belong to.
    pub plugin: String,
    /// Manifest `needs` — host-evaluated preconditions, see
    /// [`unmet_needs_blockers`].
    pub needs: Vec<Need>,
    /// Its declared settings, for routing submitted values home.
    pub settings: Vec<Field>,
}

/// `None` = nothing to probe: moss's own hosting, no deploy plugin, or one
/// that declares no setup.
pub fn deploy_target_setup(folder_path: &str) -> Option<DeployTargetSetup> {
    let plugin = current_deploy_plugin(folder_path)?;
    let (manifest, setup) = declared_setup(folder_path, &plugin, &Capability::Deploy)?;
    Some(DeployTargetSetup {
        needs: setup.needs.clone(),
        settings: manifest.declared_settings().into_iter().cloned().collect(),
        plugin,
    })
}

/// A failed manifest `need`, said as a blocker the host authored — the same
/// rendering path as the plugin's own blockers, with a remedy only moss can
/// wire: `moss:stack` submits to moss's own stack start. `check_setup` is
/// never spawned behind a failed need; the hook could only re-report what the
/// host already knows.
///
/// `stack_running` is injected because moss's local publishing stack is the
/// app's to probe; everything else a `need` can name is answered here.
pub fn unmet_needs_blockers(
    needs: &[Need],
    stack_running: &dyn Fn() -> bool,
) -> Vec<SetupBlocker> {
    blockers(needs, stack_running, &binary_on_path)
}

/// [`unmet_needs_blockers`] with both probes injected, so a test answers them
/// instead of depending on this machine's PATH and stack.
fn blockers(
    needs: &[Need],
    stack_running: &dyn Fn() -> bool,
    binary_resolves: &dyn Fn(&str) -> bool,
) -> Vec<SetupBlocker> {
    needs
        .iter()
        .filter_map(|need| match need {
            Need::Stack if !stack_running() => Some(SetupBlocker {
                id: "moss:stack".to_string(),
                message: "moss's local publishing stack is not running.".to_string(),
                form: Some(SetupForm { fields: vec![], submit: "Start the stack".to_string() }),
                field_errors: None,
            }),
            Need::Binary(name) if !binary_resolves(name) => Some(SetupBlocker {
                id: format!("moss:binary:{name}"),
                message: format!(
                    "This publish needs '{name}', which moss could not find on this \
                     computer. Install it, then publish again."
                ),
                form: None,
                field_errors: None,
            }),
            _ => None,
        })
        .collect()
}

/// Does `name` resolve to an executable on this machine's PATH? The manifest
/// declares basenames, so this asks exactly what `executeBinary` will.
fn binary_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

/// Refuse a publish whose deploy target is missing a credential it declared.
///
/// The modal is the path a person takes; this is the guarantee. A headless
/// publish never runs the frontend gate, and a plugin handed no token fails
/// deep inside its own upload with whatever the remote service says — which is
/// never "moss has no token for you".
///
/// Only declared credentials refuse here. `check_setup` is the plugin's own
/// probe: it waits indefinitely by design and its needs are answered by a
/// person choosing an action, so it belongs to the modal, not to this path.
pub fn refuse_publish(folder_path: &str) -> Result<(), String> {
    refusal(needs_from_store(folder_path)?.as_ref())
}

/// [`refuse_publish`]'s verdict, separated from reading the app-global store.
fn refusal(needs: Option<&PublishSetupNeeds>) -> Result<(), String> {
    let Some(needs) = needs else { return Ok(()) };
    let missing: Vec<&str> =
        needs.credentials.iter().filter(|c| !c.stored).map(|c| c.label.as_str()).collect();
    if missing.is_empty() {
        return Ok(());
    }
    // The remedy, not the surface. "Click Publish in moss" was true while the
    // app was the only reader; since P2b a terminal `moss deploy` takes the
    // plugin route and reads this too, and an instruction to click something
    // is a dead end for whoever is reading it there. What both readers can act
    // on is the same: the credential is entered in moss, and Publish is what
    // asks for it.
    Err(format!(
        "{} is not set up yet — moss has no {}. Open this folder in the moss app and click \
         Publish; it asks for what is missing.",
        needs.plugin_name,
        missing.join(", nor ")
    ))
}

#[cfg(test)]
#[path = "publish_setup_tests.rs"]
mod tests;
