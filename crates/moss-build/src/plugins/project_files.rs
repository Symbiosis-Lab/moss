//! Plugin-facing project/site file I/O — the headless bodies behind the
//! `plugin_config.rs` Tauri commands and the QuickJS engine's
//! file arms (open-CLI plan slice 1,
//! docs/archive/2026-08-28-open-cli-engine-and-thin-binary-plan.md).
//! Tauri-free by construction; at slice 2 this file moves verbatim into
//! `crates/moss-build/src/plugins/`, so every `crate::` path below must spell
//! the same in both crates.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::moss_paths::MossPaths;
use crate::vault::fs::PluginPath;

// ============================================================================
// File-tree entry types + home-file annotation (moved from commands.rs)
// ============================================================================

/// A file entry with home-file annotation from moss's content model.
///
/// `is_home` is true when this file is the detected "home file" for its
/// parent folder — i.e., it acts as the folder's index page.
/// Detection uses `moss_core::home::detect_home_file_in_folder()`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProjectFileEntry {
    /// Relative path from project root (e.g., "文字/无用之旅/无用之旅.md")
    pub path: String,
    /// Whether this file is the home/index file for its parent folder
    pub is_home: bool,
}

/// Annotate a flat list of relative file paths with `is_home` metadata.
///
/// Groups files by parent folder, runs `moss_core::home::detect_home_file_in_folder()`
/// per folder, and marks the winner as `is_home: true`. `root_name` is the vault
/// root's own name — the value the caller already resolved through
/// `vault::paths::VaultRoot::name()` — so a self-named home at the ROOT level
/// (`<root>/<root>.md`) is recognised. See [`annotate_home_files_marked`].
pub fn annotate_home_files(paths: &[String], root_name: &str) -> Vec<ProjectFileEntry> {
    annotate_home_files_marked(paths, &std::collections::HashSet::new(), root_name)
}

/// Marker-aware [`annotate_home_files`]: a file whose frontmatter carries
/// `home: true` (its project-relative path is in `marked`) wins as its folder's
/// home over the filename rules — matching the renderer and surviving folder
/// renames (a marked file stays the home when its name no longer matches).
///
/// `root_name` is the project root's name, already resolved by the caller via
/// `vault::paths::VaultRoot::name()`. It names the ROOT-level (`""`-parent)
/// group so the self-named election can fire there: the old `rfind('/')` on the
/// empty root-folder string handed `""` as the folder name, and the self-named
/// rule (`is_home_file(stem, "")`) could never match, silently demoting a
/// self-named root home below the priority-5 alphabetical fallback.
pub fn annotate_home_files_marked(
    paths: &[String],
    marked: &std::collections::HashSet<String>,
    root_name: &str,
) -> Vec<ProjectFileEntry> {
    use std::collections::{BTreeMap, HashSet};

    // Group filenames by parent folder. Splitting a `/`-separated relative path
    // into parent + basename is a per-FILE operation — `std::path::Path`, the
    // same idiom the 30+ other per-file basename sites use, not a hand-rolled
    // `rfind('/')` (the third basename algorithm the root-identity
    // consolidation retired).
    let mut folder_files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in paths {
        let p = std::path::Path::new(path);
        let filename = p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let parent = p
            .parent()
            .map(|par| par.to_string_lossy().into_owned())
            .unwrap_or_default();
        folder_files.entry(parent).or_default().push(filename);
    }

    // Detect home file per folder
    let mut home_paths: HashSet<String> = HashSet::new();
    for (folder, filenames) in &folder_files {
        let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
        // The folder's name for the self-named election. For the PROJECT ROOT
        // (the `""` parent) it is the owner-resolved `root_name`, never `""` —
        // otherwise a self-named root home loses to the alphabetical fallback.
        // A SUBFOLDER keeps its own basename (a per-folder name, not a root
        // identity), taken with `Path::file_name` like every other basename.
        let folder_name = if folder.is_empty() {
            root_name
        } else {
            std::path::Path::new(folder)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(folder.as_str())
        };
        // Basenames in THIS folder whose full relative path carries the marker.
        let marked_basenames: Vec<&str> = filenames
            .iter()
            .filter(|fname| {
                let full = if folder.is_empty() {
                    (*fname).clone()
                } else {
                    format!("{}/{}", folder, fname)
                };
                marked.contains(&full)
            })
            .map(|s| s.as_str())
            .collect();
        if let Some(winner) = moss_core::home::detect_home_file_in_folder_marked(
            &refs,
            folder_name,
            &marked_basenames,
        ) {
            let full_path = if folder.is_empty() {
                winner.to_string()
            } else {
                format!("{}/{}", folder, winner)
            };
            home_paths.insert(full_path);
        }
    }

    paths
        .iter()
        .map(|p| ProjectFileEntry {
            path: p.clone(),
            is_home: home_paths.contains(p.as_str()),
        })
        .collect()
}

// ============================================================================
// Path + write helpers
// ============================================================================

/// Relative paths handed to plugins and the frontend always use `/`,
/// regardless of host OS. Windows `strip_prefix` yields `\`-separated
/// paths, which would break the portable contract (a plugin that writes
/// `notes/a.md` must see `notes/a.md` back when listing) and any
/// string-keyed lookups built on `/`-shaped paths.
pub fn portable_rel_path(rel: &Path) -> String {
    moss_core::slug::normalize_separators(&rel.to_string_lossy())
}

/// Build a plugin storage file path.
///
/// Constructs the full path to a file in a plugin's storage directory:
/// `{project_path}/.moss/plugins/{plugin_name}/{relative_path}`
///
/// Unlike `build_plugin_data_path`, this does not include the "data" subdirectory.
///
/// # Arguments
/// * `project_path` - Absolute path to the project directory
/// * `plugin_name` - Name of the plugin
/// * `relative_path` - Relative path within plugin's directory
///
/// # Returns
/// The full path to the file, or an error if a symlink in the plugin's
/// storage directory would redirect it outside that directory (see
/// `PluginPath::resolve_under`).
pub fn build_plugin_storage_path(project_path: &str, plugin_name: &str, relative_path: &PluginPath) -> Result<std::path::PathBuf, String> {
    relative_path.resolve_under(
        &std::path::Path::new(project_path)
            .join(".moss")
            .join("plugins")
            .join(plugin_name),
    )
}

/// Write data to a file, creating parent directories if needed.
///
/// # Arguments
/// * `file_path` - Full path to the file
/// * `data` - Data to write (as bytes)
///
/// # Returns
/// * `Ok(())` - Success
/// * `Err(String)` - Error message
pub fn write_file_with_dirs(file_path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    // allow:raw_write plugin storage/project files under the vault root, never .moss/build/ — a fresh path each write, no evicted destination to truncate
    fs::write(file_path, data)
        .map_err(|e| format!("Failed to write file: {}", e))
}

// ============================================================================
// Plugin private-storage file I/O
// ============================================================================

/// Read a file from the plugin's private storage directory
///
/// Storage path: .moss/plugins/{plugin_name}/{relative_path}
///
/// # Arguments
/// * `plugin_name` - Name of the plugin
/// * `project_path` - Absolute path to the project directory
/// * `relative_path` - Path relative to the plugin's storage directory
///
/// # Returns
/// * `Ok(String)` - File contents
/// * `Err(String)` - Error message if read fails
/// Shared body — called by the #[tauri::command] (webview) AND the engine arm.
pub async fn read_plugin_file_impl(
    project_path: &str,
    plugin_name: &str,
    relative_path: &str,
) -> Result<String, String> {
    let stored = PluginPath::storage(plugin_name, relative_path)?;
    let file_path = build_plugin_storage_path(project_path, plugin_name, &stored)?;
    fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read file: {}", e))
}

/// Write a file to the plugin's private storage directory
///
/// Creates parent directories if they don't exist.
/// Storage path: .moss/plugins/{plugin_name}/{relative_path}
///
/// # Arguments
/// * `plugin_name` - Name of the plugin
/// * `project_path` - Absolute path to the project directory
/// * `relative_path` - Path relative to the plugin's storage directory
/// * `content` - Content to write to the file
///
/// # Returns
/// * `Ok(())` - Success
/// * `Err(String)` - Error message if write fails
/// Shared body for write_plugin_file — called by the Tauri command and the engine arm.
pub async fn write_plugin_file_impl(
    project_path: &str,
    plugin_name: &str,
    relative_path: &str,
    content: &str,
) -> Result<(), String> {
    log::info!(target: "plugin", "write_plugin_file: plugin={plugin_name} path={relative_path}");
    let outcome = async {
        let stored = PluginPath::storage(plugin_name, relative_path)?;
        let file_path = build_plugin_storage_path(project_path, plugin_name, &stored)?;
        write_file_with_dirs(&file_path, content.as_bytes())
    }.await;
    match &outcome {
        Ok(_) => log::info!(target: "plugin", "write_plugin_file ok: plugin={plugin_name} path={relative_path}"),
        Err(e) => log::warn!(target: "plugin", "write_plugin_file failed: plugin={plugin_name} path={relative_path}: {e}"),
    }
    outcome
}

/// Check if a file exists in the plugin's private storage directory
///
/// # Arguments
/// * `plugin_name` - Name of the plugin
/// * `project_path` - Absolute path to the project directory
/// * `relative_path` - Path relative to the plugin's storage directory
///
/// # Returns
/// * `Ok(bool)` - true if file exists, false otherwise
/// * `Err(String)` - Error message if check fails
/// Shared body for plugin_file_exists — called by the Tauri command and the engine arm.
pub async fn plugin_file_exists_impl(
    project_path: &str,
    plugin_name: &str,
    relative_path: &str,
) -> Result<bool, String> {
    let stored = PluginPath::storage(plugin_name, relative_path)?;
    let file_path = build_plugin_storage_path(project_path, plugin_name, &stored)?;

    Ok(file_path.exists() && file_path.is_file())
}

// ============================================================================
// Project + site file I/O
// ============================================================================

/// Write file to project root
///
/// Allows plugins to write files directly to the project root directory.
/// Useful for generating output files like syndication results.
///
/// # Arguments
/// * `project_path` - Absolute path to the project directory
/// * `plugin_id` - The calling plugin's manifest name, when known. First-party
///   callers (moss's own frontend, not a plugin) pass `None`; a plugin caller
///   always has one, which is what makes `.moss/data/social/<plugin_id>.json`
///   reachable at all — see `resolve_plugin_project_path`.
/// * `relative_path` - Relative path from project root (e.g., "output/syndication.json")
/// * `data` - File content to write
///
/// # Returns
/// * `Ok(())` - Success
/// * `Err(String)` - Error message if write fails
///
/// # Security
/// * Directory traversal (`..`) is blocked
/// * Files are scoped to project root only
/// Shared body for write_project_file — called by the Tauri command and the engine arm.
pub async fn write_project_file_impl(
    project_path: &str,
    plugin_id: Option<&str>,
    relative_path: &str,
    data: &str,
) -> Result<(), String> {
    log::info!(target: "plugin", "write_project_file: path={relative_path}");
    let outcome = async {
        let file_path = resolve_plugin_project_path(project_path, plugin_id, relative_path)?;
        write_file_with_dirs(&file_path, data.as_bytes())
    }.await;
    match &outcome {
        Ok(_) => log::info!(target: "plugin", "write_project_file ok: path={relative_path}"),
        Err(e) => log::warn!(target: "plugin", "write_project_file failed: path={relative_path}: {e}"),
    }
    outcome
}

/// Read file from project root
///
/// Allows plugins to read files directly from the project root directory.
/// Useful for reading source content or configuration files.
///
/// # Arguments
/// * `project_path` - Absolute path to the project directory
/// * `plugin_id` - The calling plugin's manifest name, when known — see
///   `write_project_file_impl`'s doc for the `None` case.
/// * `relative_path` - Relative path from project root (e.g., "posts/article.md")
///
/// # Returns
/// * `Ok(String)` - File contents
/// * `Err(String)` - Error message if read fails
///
/// # Security
/// * Directory traversal (`..`) is blocked
/// * Files are scoped to project root only
/// Shared body for read_project_file — called by the Tauri command and the engine arm.
pub async fn read_project_file_impl(
    project_path: &str,
    plugin_id: Option<&str>,
    relative_path: &str,
) -> Result<String, String> {
    let file_path = resolve_plugin_project_path(project_path, plugin_id, relative_path)?;
    fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read file: {}", e))
}

/// Resolve a plugin-supplied project-relative path for `read_project_file` /
/// `write_project_file`. Tries the narrow [`PluginPath::shared_social_data`]
/// door first — the one documented exception to the `.moss/` fence — and
/// falls back to the full [`PluginPath::sandboxed`] policy for everything
/// else, so every other `.moss/` path (identity, plugin manifests, …) is
/// refused exactly as before.
///
/// `plugin_id` gates the narrow door: only a caller with a known identity can
/// reach it, and only for that identity's own file — `None` (a first-party
/// caller) always falls straight through to the full fence.
fn resolve_plugin_project_path(
    project_path: &str,
    plugin_id: Option<&str>,
    relative_path: &str,
) -> Result<std::path::PathBuf, String> {
    if let Some(id) = plugin_id {
        if let Ok(shared) = PluginPath::shared_social_data(id, relative_path) {
            return shared.resolve_under(Path::new(project_path));
        }
    }
    PluginPath::sandboxed(relative_path)?.resolve_under(Path::new(project_path))
}

/// Read a file from the active generation directory (.moss/build/current/)
///
/// Returns the file content as base64-encoded string.
/// Used by deploy plugins to read site files without direct filesystem access.
///
/// # Arguments
/// * `relative_path` - Path relative to the current generation directory (e.g., "index.html")
///
/// # Returns
/// * `Ok(String)` - Base64-encoded file content
/// * `Err(String)` - Error message if read fails
///
/// # Security
/// * Directory traversal (`..`) is blocked
/// * Files are scoped to the active generation (.moss/build/current/) only
/// Shared body for read_site_file — called by the Tauri command and the engine
/// arm (so QuickJS plugins can read built-site bytes, not just webview callers).
pub async fn read_site_file_impl(
    project_path: &str,
    relative_path: &str,
) -> Result<String, String> {
    use base64::Engine;

    let sandboxed = PluginPath::sandboxed(relative_path)?;

    let paths = MossPaths::new(Path::new(project_path));
    let file_path = sandboxed.resolve_under(&paths.current_ptr())?;

    let bytes = fs::read(&file_path)
        .map_err(|e| format!("Failed to read site file '{}': {}", relative_path, e))?;

    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// List all files in a project directory
///
/// Recursively finds all files in the project, excluding common
/// directories that should be ignored (hidden dirs, node_modules, build outputs).
///
/// # Arguments
/// * `project_path` - Absolute path to the project directory
///
/// # Returns
/// * `Ok(Vec<String>)` - List of relative paths to all files
/// * `Err(String)` - Error message if listing fails
///
/// # Security
/// * Only reads within project directory
/// * Skips hidden directories and common build/dependency folders
/// Shared body for list_project_files — called by the Tauri command and the engine arm.
pub async fn list_project_files_impl(project_path: &str) -> Result<Vec<String>, String> {
    use walkdir::WalkDir;

    let project = Path::new(project_path);
    if !project.exists() {
        return Err(format!("Project path does not exist: {}", project_path));
    }

    let mut files = Vec::new();

    for entry in WalkDir::new(project)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            // Skip hidden directories, node_modules, build outputs
            !name.starts_with('.')
                && name != "node_modules"
                && name != "_site"
                && name != "dist"
                && name != "target"
                && name != "build"
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(rel_path) = entry.path().strip_prefix(project) {
                files.push(portable_rel_path(rel_path));
            }
        }
    }

    Ok(files)
}

/// List all files in a project directory with home-file annotations.
///
/// Like `list_project_files`, but each entry includes `is_home: bool` indicating
/// whether the file is the detected home/index file for its parent folder.
/// Uses `moss_core::home::detect_home_file_in_folder()` for consistent detection.
/// Shared body for list_project_tree — called by the Tauri command and the engine arm.
pub async fn list_project_tree_impl(project_path: &str) -> Result<Vec<ProjectFileEntry>, String> {
    use walkdir::WalkDir;

    let project = Path::new(project_path);
    if !project.exists() {
        return Err(format!("Project path does not exist: {}", project_path));
    }

    let mut files = Vec::new();

    for entry in WalkDir::new(project)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            !name.starts_with('.')
                && name != "node_modules"
                && name != "_site"
                && name != "dist"
                && name != "target"
                && name != "build"
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(rel_path) = entry.path().strip_prefix(project) {
                files.push(portable_rel_path(rel_path));
            }
        }
    }

    // Files carrying a `home: true` frontmatter marker are recognized as their
    // folder's home even when the filename doesn't match (e.g. after a folder
    // rename) — keeping plugin-facing detection consistent with the renderer.
    let marked: std::collections::HashSet<String> = files
        .iter()
        .filter(|p| p.ends_with(".md"))
        .filter(|p| crate::build::scan::scan::file_has_home_marker(&project.join(p)))
        .cloned()
        .collect();

    // The root name comes from the owner (`vault::paths`), resolved once here at
    // the boundary — so a self-named home at the project root (`<root>/<root>.md`)
    // is recognised. `annotate_home_files_marked` never re-derives it.
    let root_name = crate::vault::paths::VaultRoot::resolve(project)
        .name()
        .to_string();
    Ok(annotate_home_files_marked(&files, &marked, &root_name))
}

// ============================================================================
// Built-site file listing
// ============================================================================

/// Information about a file in the site directory.
#[derive(serde::Serialize, specta::Type)]
pub struct SiteFileInfo {
    pub path: String,
    pub size: u64,
}

/// List all files under `site_dir` with their sizes.
///
/// Returns an empty list if the directory doesn't exist.
/// This is the pure (non-Tauri) core logic, extracted for testability.
pub fn list_files_in_dir(site_dir: &std::path::Path) -> Vec<SiteFileInfo> {
    if !site_dir.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(site_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
    {
        if let Ok(rel) = entry.path().strip_prefix(site_dir) {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push(SiteFileInfo {
                path: rel.to_string_lossy().to_string(),
                size,
            });
        }
    }
    files
}

/// Shared body for list_site_files_with_sizes — called by the Tauri command and the engine arm.
pub fn list_site_files_with_sizes_impl(project_path: &std::path::Path) -> Vec<SiteFileInfo> {
    let site_dir = crate::moss_paths::MossPaths::new(project_path).current_ptr();
    list_files_in_dir(&site_dir)
}

#[cfg(test)]
mod annotate_tests {
    use super::*;

    // ============================================================================
    // annotate_home_files Tests
    // ============================================================================

    #[test]
    fn test_annotate_home_files_index_md_is_home() {
        let paths = vec![
            "index.md".to_string(),
            "about.md".to_string(),
        ];
        let result = annotate_home_files(&paths, "");
        assert_eq!(result.len(), 2);
        assert!(result[0].is_home, "index.md should be home");
        assert!(!result[1].is_home, "about.md should not be home");
    }

    #[test]
    fn test_annotate_home_files_marked_marker_beats_index() {
        // A `home: true` marked file is its folder's home even over an index.md
        // sibling (marker › index), and even though it isn't self-named —
        // the folder-rename robustness case, consistent with the renderer.
        let paths = vec![
            "articles/blog/index.md".to_string(),
            "articles/blog/intro.md".to_string(),
        ];
        let mut marked = std::collections::HashSet::new();
        marked.insert("articles/blog/intro.md".to_string());

        let result = annotate_home_files_marked(&paths, &marked, "");
        let intro = result.iter().find(|e| e.path == "articles/blog/intro.md").unwrap();
        let index = result.iter().find(|e| e.path == "articles/blog/index.md").unwrap();
        assert!(intro.is_home, "marked intro.md should be the folder home");
        assert!(!index.is_home, "index.md must not be home when a marker exists");
    }

    #[test]
    fn test_annotate_home_files_self_named_folder_note() {
        let paths = vec![
            "\u{65e0}\u{7528}\u{4e4b}\u{65c5}/\u{65e0}\u{7528}\u{4e4b}\u{65c5}.md".to_string(),
            "\u{65e0}\u{7528}\u{4e4b}\u{65c5}/\u{6cf0}\u{56fd}.md".to_string(),
        ];
        let result = annotate_home_files(&paths, "");
        assert!(result[0].is_home, "self-named folder note should be home");
        assert!(!result[1].is_home);
    }

    #[test]
    fn test_annotate_home_files_index_beats_self_named() {
        let paths = vec![
            "recipes/index.md".to_string(),
            "recipes/recipes.md".to_string(),
            "recipes/pasta.md".to_string(),
        ];
        let result = annotate_home_files(&paths, "");
        assert!(result[0].is_home, "index.md should win over self-named");
        assert!(!result[1].is_home);
    }

    #[test]
    fn test_annotate_home_files_multiple_folders() {
        let paths = vec![
            "index.md".to_string(),
            "about.md".to_string(),
            "blog/index.md".to_string(),
            "blog/post1.md".to_string(),
            "docs/README.md".to_string(),
            "docs/guide.md".to_string(),
        ];
        let result = annotate_home_files(&paths, "");
        assert!(result[0].is_home);
        assert!(!result[1].is_home);
        assert!(result[2].is_home);
        assert!(!result[3].is_home);
        assert!(result[4].is_home);
        assert!(!result[5].is_home);
    }

    #[test]
    fn test_annotate_home_files_nested_folders() {
        let paths = vec![
            "\u{6587}\u{5b57}/\u{65e0}\u{7528}\u{4e4b}\u{65c5}/\u{65e0}\u{7528}\u{4e4b}\u{65c5}.md".to_string(),
            "\u{6587}\u{5b57}/\u{65e0}\u{7528}\u{4e4b}\u{65c5}/\u{6cf0}\u{56fd}.md".to_string(),
            "\u{6587}\u{5b57}/\u{65e5}\u{672c}/index.md".to_string(),
            "\u{6587}\u{5b57}/\u{65e5}\u{672c}/\u{4eac}\u{90fd}.md".to_string(),
        ];
        let result = annotate_home_files(&paths, "");
        assert!(result[0].is_home, "self-named in nested folder");
        assert!(!result[1].is_home);
        assert!(result[2].is_home, "index.md in nested folder");
        assert!(!result[3].is_home);
    }

    #[test]
    fn test_annotate_home_files_no_home_file() {
        let paths = vec![
            "photo.jpg".to_string(),
            "style.css".to_string(),
        ];
        let result = annotate_home_files(&paths, "");
        assert!(!result[0].is_home);
        assert!(!result[1].is_home);
    }

    #[test]
    fn test_annotate_home_files_empty() {
        let paths: Vec<String> = vec![];
        let result = annotate_home_files(&paths, "");
        assert!(result.is_empty());
    }

    #[test]
    fn test_annotate_home_files_self_named_root_home() {
        // ROOT-level files (the `""` parent). The self-named root home
        // (`潮汐.md` in a project named `潮汐`) must win over the alphabetically
        // earlier sibling. Before the fix the root folder name was `""` (the
        // retired `rfind('/')` on the empty root string), so the self-named rule
        // could not fire and the priority-5 alphabetical fallback flagged
        // `aaa.md` instead. The name now arrives from the owner
        // (`VaultRoot::name()`), passed by `list_project_tree_impl`.
        let paths = vec![
            "aaa.md".to_string(),
            "\u{5728}\u{5834}.md".to_string(),
        ];
        let result = annotate_home_files_marked(
            &paths,
            &std::collections::HashSet::new(),
            "\u{5728}\u{5834}",
        );
        let home: Vec<&str> = result
            .iter()
            .filter(|e| e.is_home)
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(
            home,
            vec!["\u{5728}\u{5834}.md"],
            "the self-named root home must be elected, not the alpha-first sibling"
        );
    }
}

#[cfg(test)]
mod site_listing_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn list_files_empty_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("nonexistent");
        let result = list_files_in_dir(&missing);
        assert!(result.is_empty());
    }

    #[test]
    fn list_files_returns_files_with_sizes() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        fs::write(dir.join("a.html"), "hello").unwrap(); // 5 bytes
        fs::write(dir.join("b.css"), "body{}").unwrap(); // 6 bytes

        let mut result = list_files_in_dir(dir);
        result.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "a.html");
        assert_eq!(result[0].size, 5);
        assert_eq!(result[1].path, "b.css");
        assert_eq!(result[1].size, 6);
    }

    #[test]
    fn list_files_uses_relative_paths() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("subdir");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("file.html"), "x").unwrap();

        let result = list_files_in_dir(tmp.path());

        assert_eq!(result.len(), 1);
        // Path should be relative (subdir/file.html), not absolute
        assert_eq!(
            result[0].path,
            format!("subdir{}file.html", std::path::MAIN_SEPARATOR)
        );
        assert!(!result[0].path.starts_with('/'));
    }

    #[test]
    fn list_files_skips_directories() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("empty_dir");
        fs::create_dir_all(&sub).unwrap();

        let result = list_files_in_dir(tmp.path());
        assert!(result.is_empty());
    }

    #[test]
    fn list_files_handles_nested_structure() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        fs::write(dir.join("index.html"), "hi").unwrap();
        fs::create_dir_all(dir.join("css")).unwrap();
        fs::write(dir.join("css").join("style.css"), "body{}").unwrap();
        fs::create_dir_all(dir.join("js")).unwrap();
        fs::write(dir.join("js").join("app.js"), "()=>{}").unwrap();

        let mut result = list_files_in_dir(dir);
        result.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(result.len(), 3);
        let paths: Vec<&str> = result.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"index.html"));
    }

    #[cfg(unix)]
    #[test]
    fn list_files_follows_symlinks() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Create a real file
        fs::write(dir.join("real.html"), "content").unwrap();

        // Create a symlink to it
        symlink(dir.join("real.html"), dir.join("link.html")).unwrap();

        let mut result = list_files_in_dir(dir);
        result.sort_by(|a, b| a.path.cmp(&b.path));

        // walkdir follows symlinks by default — both files listed
        assert_eq!(result.len(), 2);
        let paths: Vec<&str> = result.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"link.html"));
        assert!(paths.contains(&"real.html"));
    }
}
