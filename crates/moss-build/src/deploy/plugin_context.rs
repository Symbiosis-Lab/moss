//! The window-free half of the plugin deploy route: the context a deploy hook
//! receives, and the one guard that can refuse a publish before it runs.
//!
//! These bodies drove `deploy_site` from `src-tauri/src/preview/commands.rs`
//! until 2026-09-09 and never touched Tauri; they moved here so the headless
//! `moss deploy` can build the same context the app does (track P, slice P1 of
//! [the plan](../../../../../docs/archive/2026-09-09-deploy-plugin-runs-windowless-plan.md)).

use crate::plugins::types::{
    DeployContext, DeployResult, HookResult, ProjectInfo, Toast, ToastOutcome,
};
use std::collections::HashMap;
use std::path::Path;

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
        },
    }
}

/// Build DeployContext for plugin execution
///
/// Note: Path fields (project_path, moss_dir, output_dir) are intentionally omitted.
/// Plugins access files through SDK functions (readSiteFile, readProjectFile, etc.)
/// which resolve paths via the runtime's internal context.
pub(crate) fn build_deploy_context(
    folder_path: &str,
    output_dir: &Path,
) -> Result<DeployContext, String> {
    // List files in site directory
    let site_files = list_site_files(output_dir)?;

    // Read custom domain from config (non-fatal — deploy proceeds without domain)
    let domain = crate::build::site_config::get_domain_config(folder_path)
        .ok()
        .and_then(|c| c.domain)
        .filter(|d| !d.is_empty());

    Ok(DeployContext {
        project_info: ProjectInfo {
            total_files: site_files.len(),
            homepage_file: None,
            folder_name: crate::vault_root::VaultRoot::resolve(folder_path).name_opt().map(str::to_string),
            site_name: None,
            lang: "en".into(),
        },
        site_files,
        config: HashMap::new(), // filled per-plugin in plugins::manager::hook_context
        domain,
    })
}

/// List files in the site directory
fn list_site_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    if dir.exists() {
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
        {
            if let Ok(rel) = entry.path().strip_prefix(dir) {
                files.push(rel.to_string_lossy().to_string());
            }
        }
    }
    Ok(files)
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
/// only** — its `deploy()` resolves owner/repo from `origin` (Phase 0,
/// `plugins/github/src/main.ts`). For every other plugin that same remote is
/// ordinary version control and says nothing about where the site ships: an
/// OnionPress site publishes to an `.onion` address that has no subpath, and
/// moss hosting serves from the root of `<site_id>.mosspub.com`.
///
/// Without that check, putting a moss vault under version control on GitHub
/// was enough to make Publish refuse. That is what happened to the William
/// Blake site: `git init` + `git remote add origin .../william-blake.git` on
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

#[cfg(test)]
#[path = "plugin_context_tests.rs"]
mod tests;
