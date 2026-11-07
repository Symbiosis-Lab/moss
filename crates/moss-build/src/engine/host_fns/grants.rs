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
/// granted basename and name the binary moss itself would run for it
/// ([`is_binary_moss_would_run`]), so a grant for `git` never runs some other
/// file called `git`. The blanket `execute_binary` token still grants every
/// name, with a deprecation warning: an installed plugin keeps working, but a
/// reviewed manifest says what it runs. Fail-closed like every gate — no
/// basename, no grant, no run.
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
    let named_grant = grants.iter().any(|entry| entry == &named);
    let blanket = grants.iter().any(|entry| entry == crate::plugins::types::EXECUTE_BINARY_GRANT);
    if !named_grant && !blanket {
        return Err(format!(
            "'execute_binary' refused: plugin '{plugin}' does not declare `{named}` \
             in manifest `requires`"
        ));
    }
    if !named_grant {
        log::warn!(
            target: "plugin",
            "plugin '{plugin}' runs '{basename}' under the deprecated blanket \
             `execute_binary` grant — name each binary in `requires` (`{named}`)"
        );
    }
    let moss_bin = crate::infra::home::moss_home().ok().map(|home| home.join("bin"));
    let path = std::env::var_os("PATH").unwrap_or_default();
    if is_binary_moss_would_run(binary_path, basename, &search_dirs(&path), moss_bin.as_deref()) {
        return Ok(());
    }
    Err(format!(
        "'execute_binary' refused: '{binary_path}' is not the `{basename}` moss runs; \
         pass the bare name `{basename}`, or the path moss resolved for it"
    ))
}

/// Where moss finds an installed binary: the PATH, then the standard install
/// directories an app launched from the Finder does not inherit. An empty or
/// relative PATH entry means the working directory, which a plugin can write,
/// so it is left out.
fn search_dirs(path: &std::ffi::OsStr) -> Vec<std::path::PathBuf> {
    std::env::split_paths(path)
        .chain(
            crate::build::assets::binary_resolver::STANDARD_INSTALL_DIRS
                .iter()
                .map(std::path::PathBuf::from),
        )
        .filter(|dir| dir.is_absolute())
        .collect()
}

/// Whether `binary_path` is the `name` moss itself would run: the bare name
/// (looked up on the PATH at spawn), `name` in one of `search_dirs`, or a file
/// called `name` under `moss_bin`, where moss keeps the tools it downloads.
/// Paths are compared with symlinks resolved, so `/bin/sh` matches wherever
/// `/bin` points; a relative path with a separator matches nothing. The check
/// trusts those directories not to change before the spawn: a plugin that can
/// write into one already chooses what runs.
fn is_binary_moss_would_run(
    binary_path: &str,
    name: &str,
    search_dirs: &[std::path::PathBuf],
    moss_bin: Option<&std::path::Path>,
) -> bool {
    if binary_path == name {
        return true;
    }
    let path = std::path::Path::new(binary_path);
    if !path.is_absolute() {
        return false;
    }
    let named = path.file_name() == Some(std::ffi::OsStr::new(name));
    let Ok(real) = path.canonicalize() else {
        return false;
    };
    search_dirs
        .iter()
        .any(|dir| dir.join(name).canonicalize().is_ok_and(|found| found == real))
        || moss_bin
            .and_then(|dir| dir.canonicalize().ok())
            .is_some_and(|dir| named && real.starts_with(dir))
}

#[cfg(test)]
mod tests {
    use super::{is_binary_moss_would_run, search_dirs};
    use std::path::PathBuf;

    fn file(dir: &std::path::Path, rel: &str) -> String {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn only_the_binary_moss_would_run_matches_a_grant() {
        let tmp = tempfile::TempDir::new().unwrap();
        let on_path = tmp.path().join("bin");
        let installed = file(tmp.path(), "bin/git");
        let elsewhere = file(tmp.path(), "project/git");
        let moss_bin = tmp.path().join("moss-bin");
        let downloaded = file(tmp.path(), "moss-bin/git-2.47/bin/git");
        let dirs = [PathBuf::from("/nonexistent"), on_path];
        let run = |p: &str| is_binary_moss_would_run(p, "git", &dirs, Some(&moss_bin));

        assert!(run("git"), "a bare name is looked up on the PATH");
        assert!(run(&installed));
        assert!(run(&downloaded), "a tool moss downloaded");
        assert!(!run(&file(tmp.path(), "moss-bin/ffmpeg")), "another tool moss downloaded");
        assert!(!run(&elsewhere), "a file named git outside where moss looks");
        assert!(!run("./git"));
        assert!(!run("project/git"));
        assert!(!run(&tmp.path().join("bin/missing/git").to_string_lossy()));
    }

    #[test]
    fn an_empty_or_relative_path_entry_is_not_searched() {
        // Absolute on every platform; "/usr/bin" is not absolute on Windows.
        let (first, last) = (std::env::temp_dir().join("usr-bin"), std::env::temp_dir().join("bin"));
        let path = std::env::join_paths([first.clone(), PathBuf::new(), PathBuf::from("bin"), last.clone()]).unwrap();
        let dirs = search_dirs(&path);
        assert!(dirs.starts_with(&[first, last]), "{dirs:?}");
        assert!(dirs.iter().all(|d| d.is_absolute()), "{dirs:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_matches_the_binary_it_resolves_to() {
        let tmp = tempfile::TempDir::new().unwrap();
        let installed = file(tmp.path(), "usr/bin/sh");
        std::os::unix::fs::symlink(tmp.path().join("usr/bin"), tmp.path().join("bin")).unwrap();
        let via_link = tmp.path().join("bin/sh");
        let dirs = [tmp.path().join("usr/bin")];

        assert!(is_binary_moss_would_run(&installed, "sh", &dirs, None));
        assert!(is_binary_moss_would_run(via_link.to_str().unwrap(), "sh", &dirs, None));
    }
}
