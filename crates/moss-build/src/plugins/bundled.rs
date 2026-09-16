//! Bundled plugin management
//!
//! This module handles plugins that are bundled with the moss application.
//! Bundled plugins are embedded in the binary at compile time using `include_dir!`
//! and are automatically copied to the project's `.moss/plugins/` directory on first use.
//!
//! ## Configuration
//!
//! Bundled plugins are configured in `build-config.toml` at the workspace root.
//! The build.rs script reads this configuration and generates constants used here.
//!
//! ## Build Time
//!
//! The `bundled-plugins/` directory is embedded in the binary:
//! ```text
//! bundled-plugins/github/
//!   ├── manifest.json
//!   ├── main.bundle.js
//!   └── icon.svg
//! ```
//!
//! ## Runtime (First Project Open)
//!
//! 1. `discover_plugins()` is called
//! 2. `ensure_bundled_plugins_installed()` auto-updates already-installed plugins
//! 3. No bundled plugin is auto-installed on new projects —
//!    users install them via the plugin installer or first-publish flow
//! 4. Once installed, bundled plugins are auto-updated
//!    when the binary contains a newer version
//! 5. Plugin now lives in project (git-tracked, customizable)
//!
//! ## Plugin Installation Policy
//!
//! For bundled plugins, we use semver-based auto-update:
//! - If plugin is a symlink → skip (development workflow)
//! - If plugin missing → install bundled version
//! - If plugin corrupted (no manifest, not symlink) → reinstall bundled
//! - If bundled version > installed version → auto-update (configurable via config.toml)
//! - If installed version > bundled version → skip (respect user's deliberate choice)
//! - If same version but different content hash → update (dev rebuild scenario)
//! - If same version and same content hash → skip (already up to date)
//!
//! Auto-update can be disabled per-project in `.moss/config.toml`:
//! ```toml
//! [plugins]
//! auto_update_plugins = false
//! ```

use include_dir::Dir;
use crate::plugins::discovery::BundledSet;
use super::install::{version_update_decision, VersionDecision};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use super::types::{Plugin, PluginManifest};

// Session-level cache to track which projects have already been checked
// This prevents redundant filesystem checks when discover_plugins() is called multiple times
static CHECKED_PROJECTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn get_checked_projects() -> &'static Mutex<HashSet<String>> {
    CHECKED_PROJECTS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The bundled set in force. The app seeds its embed at startup
/// (`startup::seed_open_half`); this crate's own tests seed the fixture tree
/// under `tests/fixtures/bundled-plugins` on first use, so every test reads
/// the same three plugins whether or not another test ran first.
fn bundled_set() -> &'static BundledSet {
    #[cfg(test)]
    return crate::plugins::discovery::seed_bundled_set(tests::fixture());
    #[cfg(not(test))]
    crate::plugins::discovery::bundled_set()
}

/// Get the list of bundled plugin names
pub fn get_bundled_plugin_names() -> &'static [&'static str] {
    bundled_set().names
}

/// Read and parse the manifest.json from a bundled plugin
fn read_bundled_manifest(plugin_name: &str) -> Option<PluginManifest> {
    let content = std::str::from_utf8(bundled_file(plugin_name, "manifest.json")?).ok()?;
    PluginManifest::parse(content).ok()
}

/// One file of a bundled plugin, by path relative to the plugin's directory
/// — or by bare name, for a bundle laid out flat.
fn bundled_file(plugin_name: &str, rel: &str) -> Option<&'static [u8]> {
    let dir = bundled_set().root.get_dir(plugin_name)?;
    dir.get_file(format!("{plugin_name}/{rel}"))
        .or_else(|| dir.files().find(|f| f.path().file_name().is_some_and(|n| n == rel)))
        .map(|f| f.contents())
}

/// Read and parse the manifest.json from an installed plugin
pub fn read_installed_manifest(plugin_path: &Path) -> Option<PluginManifest> {
    let manifest_path = plugin_path.join("manifest.json");
    let content = fs::read_to_string(&manifest_path).ok()?;
    PluginManifest::parse(&content).ok()
}

/// Compute SHA-256 hash of embedded bundled plugin files.
///
/// Recursively hashes all files in the bundled plugin directory,
/// using sorted relative paths for deterministic ordering.
/// Paths are made relative to the plugin directory so they match
/// the paths produced by `compute_installed_hash`.
fn compute_bundled_hash(plugin_name: &str) -> Option<String> {
    let plugin_dir = bundled_set().root.get_dir(plugin_name)?;
    let base_path = plugin_dir.path();
    let mut hasher = Sha256::new();

    // Collect all files recursively with their relative paths
    let mut files: Vec<(&Path, &[u8])> = Vec::new();
    collect_bundled_files(plugin_dir, &mut files);

    // Make paths relative to the plugin dir and sort for deterministic ordering
    let mut rel_files: Vec<(String, &[u8])> = files
        .into_iter()
        .map(|(path, contents)| {
            let rel = path.strip_prefix(base_path).unwrap_or(path);
            (rel.to_string_lossy().into_owned(), contents)
        })
        .collect();
    rel_files.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (rel_path, contents) in &rel_files {
        // Hash the relative path and file contents together
        hasher.update(rel_path.as_bytes());
        hasher.update(contents);
    }

    Some(format!("{:x}", hasher.finalize()))
}

/// Recursively collect files from an embedded Dir
fn collect_bundled_files<'a>(dir: &'a Dir<'a>, files: &mut Vec<(&'a Path, &'a [u8])>) {
    for file in dir.files() {
        files.push((file.path(), file.contents()));
    }
    for subdir in dir.dirs() {
        collect_bundled_files(subdir, files);
    }
}

/// Compute SHA-256 hash of installed plugin's **code files** on disk.
///
/// Only hashes files whose relative paths match files in the bundled plugin.
/// This excludes data files (config.toml, config.json, syndicated.json, etc.)
/// written by plugins at runtime, preventing false hash mismatches that would
/// trigger unnecessary updates.
///
/// Returns None if the directory doesn't exist or can't be read.
fn compute_installed_hash(plugin_name: &str, plugin_dir: &Path) -> Option<String> {
    if !plugin_dir.exists() {
        return None;
    }

    // Get the set of relative paths from the bundled plugin to use as a filter
    let bundled_paths = collect_bundled_file_paths(plugin_name)?;

    let mut hasher = Sha256::new();

    // Collect all files recursively with their relative paths
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    collect_installed_files(plugin_dir, plugin_dir, &mut files).ok()?;

    // Filter to only code files (those that exist in the bundle)
    files.retain(|(rel_path, _)| bundled_paths.contains(rel_path.as_str()));

    // Sort by relative path for deterministic ordering
    files.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (rel_path, contents) in &files {
        // Hash the relative path and file contents together
        hasher.update(rel_path.as_bytes());
        hasher.update(contents);
    }

    Some(format!("{:x}", hasher.finalize()))
}

/// SHA-256 over what the engine executes: `manifest.json` and the entry file
/// it names, and nothing else.
///
/// One hash for both questions the admission gate asks — is this the code
/// moss ships, and is this the code the user allowed — so the two can never
/// be answered about different bytes. Not the whole directory: plugins write
/// their own data files beside the code (`write_plugin_file_impl`), and
/// hashing those would withdraw consent every time the plugin saved
/// something. `None` when either file cannot be read, which no caller treats
/// as a match.
pub fn code_hash(plugin_dir: &Path, manifest: &PluginManifest) -> Option<String> {
    let manifest_bytes = fs::read(plugin_dir.join("manifest.json")).ok()?;
    let entry_bytes = fs::read(plugin_dir.join(&manifest.entry)).ok()?;
    Some(hash_code(&manifest_bytes, &manifest.entry, &entry_bytes))
}

/// [`code_hash`] of the copy this binary ships under `plugin_name`, or
/// `None` when it ships none. The exact form of "moss shipped these bytes":
/// id and version cannot tell the bundle's copy from a sideloaded one
/// carrying the same manifest, and that is the whole question when deciding
/// what vouches for a plugin.
pub fn bundled_code_hash(plugin_name: &str) -> Option<String> {
    let manifest = read_bundled_manifest(plugin_name)?;
    let manifest_bytes = bundled_file(plugin_name, "manifest.json")?;
    let entry_bytes = bundled_file(plugin_name, &manifest.entry)?;
    Some(hash_code(manifest_bytes, &manifest.entry, entry_bytes))
}

fn hash_code(manifest_bytes: &[u8], entry: &str, entry_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    for (name, bytes) in [("manifest.json", manifest_bytes), (entry, entry_bytes)] {
        hasher.update(name.as_bytes());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

/// Collect the set of relative file paths in a bundled plugin.
///
/// Returns None if the plugin is not found in the bundle.
fn collect_bundled_file_paths(plugin_name: &str) -> Option<HashSet<String>> {
    let plugin_dir = bundled_set().root.get_dir(plugin_name)?;
    let base_path = plugin_dir.path();
    let mut files: Vec<(&Path, &[u8])> = Vec::new();
    collect_bundled_files(plugin_dir, &mut files);

    let paths: HashSet<String> = files
        .into_iter()
        .map(|(path, _)| {
            let rel = path.strip_prefix(base_path).unwrap_or(path);
            rel.to_string_lossy().into_owned()
        })
        .collect();

    Some(paths)
}

/// Recursively collect files from a directory on disk
fn collect_installed_files(
    base: &Path,
    dir: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_installed_files(base, &path, files)?;
        } else {
            let rel_path = path.strip_prefix(base).unwrap_or(&path);
            let contents = fs::read(&path)?;
            files.push((rel_path.to_string_lossy().into_owned(), contents));
        }
    }
    Ok(())
}

/// Read the `auto_update_plugins` setting from `.moss/config.toml`.
///
/// Looks for `[plugins] auto_update_plugins = true/false`.
/// Defaults to `true` if the config file or section is missing.
fn read_auto_update_plugins_config(project_path: &str) -> bool {
    let config_path = Path::new(project_path)
        .join(".moss")
        .join("config.toml");

    if !config_path.exists() {
        return true; // default: auto-update enabled
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return true,
    };

    let config: toml::Value = match toml::from_str(&content) {
        Ok(c) => c,
        Err(_) => return true,
    };

    config
        .get("plugins")
        .and_then(|p| p.get("auto_update_plugins"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Check if a plugin should be installed/updated
///
/// Returns true if:
/// - Plugin is not installed (directory missing)
/// - Plugin exists but corrupted (no valid manifest) AND it's not a symlink
/// - Bundled version is newer than installed (semver auto-update, when enabled)
/// - Plugin has same version as bundled but different content (dev rebuild)
///
/// Returns false (respects user installation) if:
/// - Installed version is newer than bundled (user deliberately installed newer version)
/// - Auto-update disabled and bundled is newer
/// - Plugin has same version and same content (already up to date)
/// - Plugin is a symlink (development workflow)
fn should_install_plugin(plugin_name: &str, target_dir: &Path, auto_update: bool) -> bool {
    // Check if we have a bundled manifest to install from
    let bundled_manifest = match read_bundled_manifest(plugin_name) {
        Some(m) => m,
        None => {
            log::warn!(target: "plugin", "⚠️ [BUNDLED] No manifest found in bundle for '{}'", plugin_name);
            return false;
        }
    };

    // Plugin doesn't exist - install it
    if !target_dir.exists() {
        log::info!(target: "plugin", "✅ [BUNDLED] Plugin '{}' not installed, will install bundled version",
            plugin_name);
        return true;
    }

    // Respect symlinks (development workflow)
    if target_dir.is_symlink() {
        log::debug!(target: "plugin", "[BUNDLED] Plugin '{}' is a symlink, respecting developer setup",
            plugin_name);
        return false;
    }

    // Check for valid installed manifest
    let installed_manifest = match read_installed_manifest(target_dir) {
        Some(m) => m,
        None => {
            // Directory exists but corrupted (no valid manifest, not a symlink) - reinstall
            log::info!(target: "plugin", "✅ [BUNDLED] Plugin '{}' corrupted (no valid manifest), reinstalling",
                plugin_name);
            return true;
        }
    };

    // Compare versions using semver to decide update/skip/hash-check
    match version_update_decision(&installed_manifest.version, &bundled_manifest.version, auto_update) {
        VersionDecision::Update => {
            log::info!(target: "plugin", "✅ [BUNDLED] Plugin '{}' has older version {} (bundled: {}), auto-updating",
                plugin_name, installed_manifest.version, bundled_manifest.version);
            return true;
        }
        VersionDecision::Skip => {
            log::debug!(target: "plugin", "[BUNDLED] Plugin '{}' has version {} (bundled: {}), skipping",
                plugin_name, installed_manifest.version, bundled_manifest.version);
            return false;
        }
        VersionDecision::CompareHash => {
            // Fall through to content hash comparison below
        }
    }

    // Same version — compare content hashes to detect dev rebuild scenario
    let bundled_hash = compute_bundled_hash(plugin_name);
    let installed_hash = compute_installed_hash(plugin_name, target_dir);

    match (bundled_hash, installed_hash) {
        (Some(bh), Some(ih)) if bh == ih => {
            log::debug!(target: "plugin", "[BUNDLED] Plugin '{}' is up-to-date (same version and content)",
                plugin_name);
            false
        }
        (Some(bh), Some(ih)) => {
            log::info!(target: "plugin", "✅ [BUNDLED] Plugin '{}' has same version {} but different content (bundled: {}..., installed: {}...), updating",
                plugin_name, installed_manifest.version, bh.get(..8).unwrap_or(&bh), ih.get(..8).unwrap_or(&ih));
            true
        }
        _ => {
            // Can't compute hash (shouldn't happen for valid plugin) — skip to be safe
            log::debug!(target: "plugin", "[BUNDLED] Plugin '{}' hash comparison inconclusive, skipping",
                plugin_name);
            false
        }
    }
}

/// Check if a bundled plugin has the deploy capability (test-only)
#[cfg(test)]
fn is_deployer(plugin_name: &str) -> bool {
    read_bundled_manifest(plugin_name)
        .map(|m| m.capabilities.contains(&super::types::Capability::Deploy))
        .unwrap_or(false)
}

/// Get metadata for a bundled plugin (for registry)
pub fn get_bundled_plugin_info(plugin_name: &str) -> Option<PluginManifest> {
    read_bundled_manifest(plugin_name)
}

/// Get the icon SVG content for a bundled plugin
///
/// Returns the raw SVG content if the plugin has an icon.svg file.
/// Returns None if the plugin doesn't exist or has no icon.
pub fn get_bundled_plugin_icon(plugin_name: &str) -> Option<String> {
    let plugin_dir = bundled_set().root.get_dir(plugin_name)?;

    // Try to find icon.svg in the plugin directory
    let icon_file = plugin_dir.files().find(|f| {
        f.path().file_name().map(|n| n == "icon.svg").unwrap_or(false)
    })?;

    std::str::from_utf8(icon_file.contents()).ok().map(|s| s.to_string())
}

/// Embedded bundle JS for a bundled plugin (tests + Phase-3 verification).
/// NOTE (sdk-bundles probe R4): the EMBEDDED copy equals the installed
/// `.moss/plugins/<name>/main.bundle.js` for fresh installs; production
/// BundleSource reads the INSTALLED copy. Tests pin the embedded bytes.
pub fn get_bundled_plugin_bundle_source(plugin_name: &str) -> Option<&'static str> {
    let dir = bundled_set().root.get_dir(plugin_name)?;
    let file = dir.get_file(format!("{plugin_name}/main.bundle.js"))?;
    std::str::from_utf8(file.contents()).ok()
}

/// Install a specific bundled plugin to the project
///
/// This is called by the registry for user-initiated installs.
/// It extracts the plugin from the embedded binary to .moss/plugins/
pub fn install_bundled_plugin(plugin_name: &str, project_path: &str) -> Result<(), String> {
    let plugins_dir = Path::new(project_path).join(".moss/plugins");
    let target_dir = plugins_dir.join(plugin_name);

    // Get the bundled plugin directory
    let plugin_dir = bundled_set().root.get_dir(plugin_name)
        .ok_or_else(|| format!("Plugin '{}' not found in bundle", plugin_name))?;

    // Create plugins directory if needed
    fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

    // Extract plugin files, overwriting code files but preserving data files
    // (config.toml, config.json, syndicated.json, etc.) written by plugins at runtime
    log::info!(target: "plugin", "✅ [BUNDLED] Installing plugin: {}", plugin_name);
    extract_plugin_dir(plugin_dir, &target_dir)?;
    log::info!(target: "plugin", "✅ [BUNDLED] Installed: {}", plugin_name);

    Ok(())
}

/// Ensures already-installed bundled plugins are up-to-date in `.moss/plugins/`.
///
/// This function is called during plugin discovery. No bundled plugin is
/// auto-installed on new projects — users install them from the catalog
/// (preview-declared plugins appear there once preview features are on) or by
/// dev sideload. Once installed, `should_install_plugin()` handles
/// version/hash checks and `extract_plugin_dir()` preserves user config files.
///
/// # Arguments
/// * `project_path` - Absolute path to the project root
///
/// # Returns
/// * `Ok(())` - All installed plugins checked/updated successfully
/// * `Err(String)` - Update failed with error message
pub fn ensure_bundled_plugins_installed(project_path: &str) -> Result<(), String> {
    // Check session cache to avoid redundant checks
    {
        let checked = get_checked_projects().lock().map_err(|e| e.to_string())?;
        if checked.contains(project_path) {
            // Already checked this project this session, skip
            return Ok(());
        }
    }

    // Read auto-update config once for all plugins
    let auto_update = read_auto_update_plugins_config(project_path);

    let plugins_dir = Path::new(project_path).join(".moss/plugins");

    for plugin_name in bundled_set().names {
        let target_dir = plugins_dir.join(plugin_name);

        // Only auto-update plugins that are already installed.
        // Users install new plugins via the plugin installer or first-publish flow.
        if !target_dir.exists() {
            log::debug!(target: "plugin",
                "[BUNDLED] Skipping '{}' (not installed, use plugin installer)",
                plugin_name);
            continue;
        }

        // Check if we need to install/update (version checking)
        if !should_install_plugin(plugin_name, &target_dir, auto_update) {
            log::debug!(target: "plugin", "[BUNDLED] Plugin '{}' is up-to-date, skipping",
                plugin_name);
            continue;
        }

        // Extract from embedded directory
        if let Some(plugin_dir) = bundled_set().root.get_dir(plugin_name) {
            // Create plugins directory if needed
            fs::create_dir_all(&plugins_dir)
                .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

            // Extract plugin files, overwriting code files but preserving data files
            // (config.toml, config.json, syndicated.json, etc.) written by plugins at runtime
            log::info!(target: "plugin", "✅ [BUNDLED] Installing/updating bundled plugin: {}", plugin_name);
            extract_plugin_dir(plugin_dir, &target_dir)?;
            log::info!(target: "plugin", "✅ [BUNDLED] Installed: {}", plugin_name);
        } else {
            log::warn!(target: "plugin", "⚠️ [BUNDLED] Plugin '{}' not found in bundle",
                plugin_name);
        }
    }

    // Mark this project as checked for this session
    {
        let mut checked = get_checked_projects().lock().map_err(|e| e.to_string())?;
        checked.insert(project_path.to_string());
    }

    Ok(())
}

/// Recursively extract a plugin directory from the embedded bundle
fn extract_plugin_dir(source: &Dir, target: &Path) -> Result<(), String> {
    // Create target directory
    fs::create_dir_all(target)
        .map_err(|e| format!("Failed to create directory {}: {}", target.display(), e))?;

    // Extract files
    for file in source.files() {
        // Get just the filename, not the full path
        let filename = file
            .path()
            .file_name()
            .ok_or_else(|| format!("Invalid file path in bundle: {:?}", file.path()))?;
        let file_path = target.join(filename);

        // allow:raw_write first-use install copies the bundle under .moss/plugins/, not build output
        fs::write(&file_path, file.contents())
            .map_err(|e| format!("Failed to write {}: {}", file_path.display(), e))?;

        log::debug!(target: "plugin", "[BUNDLED] Extracted: {}", file_path.display());
    }

    // Recursively extract subdirectories
    for dir in source.dirs() {
        let dir_name = dir
            .path()
            .file_name()
            .ok_or_else(|| format!("Invalid directory path in bundle: {:?}", dir.path()))?;
        extract_plugin_dir(dir, &target.join(dir_name))?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "bundled_tests.rs"]
mod tests;

// ── The installing discovery wrappers ────────────────────────────────────────
// The engine's read path is `discovery::discover_installed_plugins` /
// `get_deploy_plugin_readonly`; these install the bundled set first. The
// embedded payloads stay in the app: this crate only ever sees the seeded set.

/// Discover all plugins in a project's .moss/plugins directory
///
/// This function also ensures bundled plugins are installed before discovery.
/// Bundled plugins are embedded in the moss binary and automatically copied
/// to the project's `.moss/plugins/` directory on first use.
///
/// # Arguments
/// * `project_path` - Absolute path to the project folder
///
/// # Returns
/// * `Ok(Vec<Plugin>)` - List of discovered plugins
/// * `Err(String)` - Error message if discovery fails
pub fn discover_plugins(project_path: &str) -> Result<Vec<Plugin>, String> {
    // First, ensure bundled plugins are installed
    // This copies any missing bundled plugins to .moss/plugins/
    if let Err(e) = ensure_bundled_plugins_installed(project_path) {
        log::warn!(target: "plugin", "⚠️ Failed to install bundled plugins: {}", e);
        // Continue with discovery - bundled plugins are nice-to-have, not required
    }

    super::discovery::discover_installed_plugins(project_path)
}

pub fn get_deploy_plugin(project_path: &str) -> Result<Option<String>, String> {
    super::discovery::resolve_deploy_plugin(project_path, &discover_plugins(project_path)?)
}


#[cfg(test)]
mod install_discovery_tests {
    use std::fs;
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discover_plugins_no_auto_install_on_fresh_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // No bundled plugins should be auto-installed on a fresh project
        let result = discover_plugins(project_path);
        assert!(result.is_ok());
        let plugins = result.unwrap();
        assert!(
            plugins.is_empty(),
            "Expected no plugins on fresh project, got {}",
            plugins.len()
        );
    }
}

