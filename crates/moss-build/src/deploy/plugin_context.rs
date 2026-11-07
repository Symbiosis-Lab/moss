//! The window-free half of the plugin deploy route: the context a deploy hook
//! receives, the copy of the site it publishes from, and the one guard that
//! can refuse a publish before it runs. Shared by the app and `moss deploy`.

use crate::plugins::types::{
    DeployContext, DeployResult, HookResult, ProjectInfo, Toast, ToastOutcome,
};
use crate::moss_paths::MossPaths;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// A deploy that failed before the plugin ran: `message` for the log, `title`
/// for the user.
pub(crate) fn deploy_failed(message: String, title: String) -> DeployResult {
    DeployResult::Completed {
        result: HookResult {
            success: false,
            message: Some(message),
            toast: Some(Toast { outcome: ToastOutcome::Error, title, url: None }),
            context: None,
            deployment: None,
            setup: None,
            copy: None,
            offer: None,
        },
    }
}

/// The custom domain the folder's config names, if any.
pub(crate) fn deploy_domain(folder_path: &str) -> Option<String> {
    crate::build::site_config::get_domain_config(folder_path)
        .ok()
        .and_then(|c| c.domain)
        .filter(|d| !d.is_empty())
}

/// The deploy hook's context, for a copy of the site already made.
pub(crate) fn build_deploy_context(
    folder_path: &str,
    scratch: &DeployScratch,
    domain: Option<String>,
) -> DeployContext {
    DeployContext {
        project_info: ProjectInfo {
            total_files: scratch.site_files.len(),
            homepage_file: None,
            folder_name: crate::vault_root::VaultRoot::resolve(folder_path).name_opt().map(str::to_string),
            site_name: None,
            lang: "en".into(),
        },
        site_files: scratch.site_files.clone(),
        site_dir: scratch.site_dir.to_string_lossy().into_owned(),
        index_file: scratch.index_file.to_string_lossy().into_owned(),
        config: HashMap::new(), // filled per-plugin in plugins::manager::hook_context
        domain,
    }
}

/// Read the git origin remote URL from .git/config.
pub(crate) fn get_git_origin(folder_path: &str) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(folder_path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("No git origin".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The GitHub Pages plugin's id (its manifest `name`). Lives here rather than
/// beside `deploy::MOSS_TARGET_ID` because this guard is its only consumer.
const GITHUB_DEPLOY_PLUGIN: &str = "github";

/// Whether Publish must refuse: GitHub Pages would serve this site from a
/// subpath, where moss's absolute paths (`/style.css`, `/article/`) 404.
///
/// All three conditions are required, and the plugin is the one that was
/// missing. The `origin` remote is the deploy target for the **github plugin
/// only** — its `deploy()` resolves owner/repo from `origin` (see
/// `plugins/github/src/main.ts`). For every other plugin that same remote is
/// ordinary version control and says nothing about where the site ships: an
/// OnionPress site publishes to an `.onion` address that has no subpath, and
/// moss hosting serves from the root of `<site_id>.mosspub.com`.
///
/// Without that check, putting a moss vault under version control on GitHub
/// was enough to make Publish refuse. That is what happened to a real
/// site: `git init` + `git remote add origin .../my-site.git` on
/// 2026-08-02 silently disabled publishing to its onion, and every attempt
/// after that failed with "Custom domain required" while `[hooks] deploy =
/// "onionpress"` was pinned in config.toml.
///
/// `deploy_plugin` is the app's `deploy::current_deploy_plugin` answer — the
/// same resolution `PluginManager::execute_deploy` uses to pick the plugin that
/// will run, so the guard can never police a deploy that isn't the one about to
/// happen.
/// `None` — moss's own hosting — lets the publish through to fail on its own
/// terms.
pub(crate) fn refuses_subpath_pages(
    deploy_plugin: Option<&str>,
    domain: Option<&str>,
    origin: Option<&str>,
) -> bool {
    deploy_plugin == Some(GITHUB_DEPLOY_PLUGIN)
        && domain.is_none()
        && origin.is_some_and(is_non_root_github_pages)
}

/// Check if a GitHub origin URL is a non-root Pages repo.
/// Root repos are `username.github.io` (no project path).
/// Project repos are `username/repo-name` → served at `username.github.io/repo-name/`.
fn is_non_root_github_pages(origin: &str) -> bool {
    let normalized = origin.trim_end_matches('/').trim_end_matches(".git");

    // Extract repo name from various URL formats
    let repo_name = if let Some(rest) = normalized.strip_prefix("git@github.com:") {
        rest.rsplit('/').next()
    } else if normalized.contains("github.com/") {
        normalized.rsplit('/').next()
    } else {
        return false; // Not a GitHub repo
    };

    match repo_name {
        Some(name) if !name.is_empty() => !name.ends_with(".github.io"),
        _ => false,
    }
}

/// The directory a deploy hook publishes from: exactly the files this publish
/// ships, copied (a reflink where the filesystem has them) out of the sealed
/// generation, read-only, and removed on drop. A hook reads this rather than
/// `current`, which also holds the pages built for preview only and which
/// the next build can advance mid-publish.
///
/// Made under the folder's `deploy` lock, held until drop, so one publish of
/// a folder runs at a time and everything already in
/// `.moss/build.nosync/deploy/` when the lock is taken was left by one that
/// crashed. Lock order: the deploy lock is never taken while the build lock
/// is held (a build takes it only inside a render or a promotion, and nothing
/// a build runs takes the deploy lock), so a publish waiting here cannot
/// deadlock against a build. That directory holds:
/// - `<generation>/`: the site.
/// - `<generation>.index`: a path the hook may hand git as `GIT_INDEX_FILE`.
pub(crate) struct DeployScratch {
    site_files: Vec<String>,
    site_dir: PathBuf,
    index_file: PathBuf,
    // Declared last: fields drop after `Drop::drop`, so the copy is gone
    // before the next publish can take the lock.
    _lock: crate::infra::folder_lock::Held,
}

impl DeployScratch {
    /// Blocking: waits for another publish of `folder` to finish (calling
    /// `on_wait` first if it has to), then copies `site_files` out of
    /// `generation`, unless `cancelled` was raised meanwhile.
    pub(crate) fn create(
        folder: &Path,
        generation: &str,
        site_files: Vec<String>,
        on_wait: impl FnOnce(),
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, String> {
        let lock = crate::infra::folder_lock::acquire_named_noting_wait(folder, "deploy", on_wait)?;
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("the publish was cancelled".to_string());
        }
        let mp = MossPaths::new(folder);
        let from = mp.generation_dir(generation);
        if !from.is_dir() {
            // Another process's build retired it while this one waited.
            return Err("this site was rebuilt while waiting for another publish; publish again".to_string());
        }
        let root = mp.deploy_scratch_dir();
        remove_tree(&root).map_err(|e| format!("could not clear {} for the deploy plugin: {e}", root.display()))?;
        let scratch = Self {
            site_dir: root.join(generation),
            index_file: root.join(format!("{generation}.index")),
            site_files,
            _lock: lock,
        };
        let copy = || -> io::Result<()> {
            std::fs::create_dir_all(&root)?;
            std::fs::create_dir(&scratch.site_dir)?;
            for rel in &scratch.site_files {
                let to = scratch.site_dir.join(rel);
                crate::build::io_utils::copy_output(&from.join(rel), &to)?;
                set_readonly(&to, true)?;
            }
            Ok(())
        };
        copy().map_err(|e| format!("could not copy the site for the deploy plugin: {e}"))?;
        Ok(scratch)
    }
}

impl Drop for DeployScratch {
    fn drop(&mut self) {
        if let Some(root) = self.site_dir.parent() {
            if let Err(e) = remove_tree(root) {
                log::warn!("could not remove the deploy copy at {}: {e}", root.display());
            }
        }
    }
}

fn set_readonly(path: &Path, readonly: bool) -> io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_readonly(readonly);
    std::fs::set_permissions(path, perms)
}

/// `remove_dir_all`, after making the files writable again: Windows refuses
/// to delete a read-only file. A directory that is not there is removed.
fn remove_tree(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    if cfg!(windows) {
        for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
            if entry.file_type().is_file() {
                let _ = set_readonly(entry.path(), false);
            }
        }
    }
    std::fs::remove_dir_all(dir)
}

#[cfg(test)]
#[path = "plugin_context_tests.rs"]
mod tests;
