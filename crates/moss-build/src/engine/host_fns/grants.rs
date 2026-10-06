//! The host-capability gates: what a plugin's LOADED manifest granted it,
//! and the refusal every arm shares. Split from `host_fns.rs` 2026-08-30
//! when the named-binary gate pushed that file over the prod-line ceiling.
//!
//! [`require_binary_grant`] refuses `execute_binary` — the one privileged host
//! command — unless the calling plugin's manifest declares the binary in
//! `requires`. (A generic `require_grant` existed while the blanket token was
//! the grammar; the named form was its last consumer, so it is gone until a
//! second gated command earns it back.)
//!
//! FAIL-CLOSED by construction: an unknown calling plugin, a plugin missing from
//! the discovered set, or an absent declaration all deny. A privileged capability
//! must never be granted by the *absence* of information.
//!
//! PURE — it reads only what the caller was bound with. Re-reading
//! `.moss/plugins/<name>/manifest.json` here would derive authority from a file
//! the running app can rewrite; the grant set comes from [`GrantRegistry`] and is
//! fixed at discovery.
//!
//! Gated commands are those that escape the QuickJS sandbox in a way the user
//! cannot undo — today just `execute_binary` (arbitrary native processes).
//! Using a key from the keystore is NOT gated: a caller signs only with its
//! own scoped key, which spends nothing of the user's or another plugin's.

use super::EngineHost;

/// Capabilities each DISCOVERED plugin declared, snapshotted from the manifests
/// the manager parsed at discovery and re-snapshotted on `rediscover_plugins`.
/// Cloning shares one snapshot (Arc), so the manager and the engine's host
/// dispatch task never disagree.
///
/// `.moss/plugins/<name>/manifest.json` stays writable while moss runs, so it is
/// NEVER the authority for a grant decision. A plugin absent from the snapshot
/// resolves to `None` and is denied — it is not looked up on disk.
#[derive(Clone, Default)]
pub struct GrantRegistry {
    by_plugin: std::sync::Arc<
        std::sync::RwLock<std::collections::HashMap<String, Declared>>,
    >,
}

/// The manifest declarations a host arm may gate on, snapshotted together and
/// handed to the dispatch as ONE resolution — the alternative was a parallel
/// `Option<Vec>` per question, each resolved and threaded separately.
///
/// `requires` is the host-capability allowlist [`require_binary_grant`] reads.
/// `user_supplied_secrets` are the credential keys the USER fills in, which
/// the write arm refuses — a plugin deposits what its login returned, and
/// nothing a person typed. `plugin_owned_secrets` are the `hidden` secret
/// settings — declared custody, the keys that arm admits.
#[derive(Clone, Debug, Default)]
pub struct Declared {
    pub requires: Vec<String>,
    pub user_supplied_secrets: Vec<String>,
    pub plugin_owned_secrets: Vec<String>,
    /// `contributes.jobs`: the verb/noun a plugin proposes for each job id, so
    /// the lifecycle arm can stamp a task's receipt without asking the app.
    pub jobs: std::collections::HashMap<String, crate::plugins::contributions::JobDescriptor>,
}

impl GrantRegistry {
    pub fn from_plugins(plugins: &[crate::plugins::types::Plugin]) -> Self {
        let registry = Self::default();
        registry.replace(plugins);
        registry
    }

    /// Re-snapshot after an install/uninstall changes the plugin set. Skipping
    /// this leaves a newly installed plugin UNGRANTED, never over-granted.
    pub fn replace(&self, plugins: &[crate::plugins::types::Plugin]) {
        let next = plugins
            .iter()
            .map(|p| {
                (
                    p.manifest.name.clone(),
                    Declared {
                        requires: p.manifest.requires.clone().unwrap_or_default(),
                        user_supplied_secrets: p.manifest.user_supplied_secret_keys(),
                        plugin_owned_secrets: p.manifest.plugin_owned_secret_keys(),
                        jobs: p
                            .manifest
                            .contributes
                            .as_ref()
                            .and_then(|c| c.jobs.as_ref())
                            .map(|jobs| jobs.descriptors.clone())
                            .unwrap_or_default(),
                    },
                )
            })
            .collect();
        // Poison recovery over silent staleness: a stale snapshot would keep
        // granting a capability an uninstalled plugin no longer holds.
        *self
            .by_plugin
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    }

    /// `None` = not in the discovered set; every gated capability then denies.
    pub fn declared_for(&self, plugin: &str) -> Option<Declared> {
        self.by_plugin
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(plugin)
            .cloned()
    }
}


/// The caller's identity and declared set, or the refusal — shared by every
/// gate so fail-closed is written once.
pub(super) fn caller_declared<'h>(
    host: &'h EngineHost<'_>,
    cmd: &str,
) -> std::result::Result<(&'h str, &'h Declared), String> {
    let plugin = host.plugin.ok_or_else(|| {
        format!("'{cmd}' is a gated capability and the calling plugin could not be identified — refusing")
    })?;
    let declared = host.declared.ok_or_else(|| {
        format!("'{cmd}' refused for plugin '{plugin}': not in the discovered plugin set")
    })?;
    Ok((plugin, declared))
}

/// The `execute_binary` gate, per binary: `requires` names each executable as
/// `execute_binary:<basename>`, and the call's `binaryPath` must carry a
/// granted basename. The blanket `execute_binary` token predates the naming
/// (2026-08-30) and still grants every binary, with a deprecation warning:
/// an installed plugin keeps working, but a reviewed manifest says what it
/// runs. Fail-closed like every gate — no basename, no grant, no run.
pub(super) fn require_binary_grant(
    host: &EngineHost<'_>,
    binary_path: &str,
) -> std::result::Result<(), String> {
    let (plugin, declared) = caller_declared(host, "execute_binary")?;
    let grants = &declared.requires;
    let basename = std::path::Path::new(binary_path)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| {
            format!("'execute_binary' refused: '{binary_path}' has no resolvable basename")
        })?;
    let named = format!("{}:{basename}", crate::plugins::types::EXECUTE_BINARY_GRANT);
    if grants.iter().any(|entry| entry == &named) {
        return Ok(());
    }
    if grants.iter().any(|entry| entry == crate::plugins::types::EXECUTE_BINARY_GRANT) {
        log::warn!(
            target: "plugin",
            "plugin '{plugin}' runs '{basename}' under the deprecated blanket \
             `execute_binary` grant — name each binary in `requires` (`{named}`)"
        );
        return Ok(());
    }
    Err(format!(
        "'execute_binary' refused: plugin '{plugin}' does not declare `{named}` \
         in manifest `requires`"
    ))
}
