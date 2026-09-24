//! Plugin discovery and loading

use super::types::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The set of installed channels read from `[channels.<id>]` tables in `.moss/config.toml`.
/// Presence of a key = installed. Value holds per-channel settings (may be empty).
///
/// See `docs/reference/channels.md`.
#[derive(Debug, Default, Clone)]
pub struct ChannelsConfig {
    pub channels: BTreeMap<String, toml::value::Table>,
}

impl ChannelsConfig {
    pub fn is_installed(&self, id: &str) -> bool {
        self.channels.contains_key(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.channels.keys().map(|s| s.as_str())
    }
}

/// The plugins a binary carries. The app builds one from its compile-time
/// embed (`plugins::bundled_embed`) and seeds it once; the engine reads the
/// default deployer from it. Unseeded — unit tests, a CLI that embeds nothing
/// — there is no bundle and no default deployer, and a plugin is whatever is
/// already installed under `.moss/plugins/`.
pub struct BundledSet {
    /// The bundled plugin ids, in the build script's order.
    pub names: &'static [&'static str],
    /// The embedded `bundled-plugins/` tree: one directory per id.
    pub root: &'static include_dir::Dir<'static>,
    /// The deployer `[hooks] deploy` defaults to; empty for none.
    pub default_deployer: &'static str,
}

static BUNDLED_SET: std::sync::OnceLock<BundledSet> = std::sync::OnceLock::new();

static NO_DIR: include_dir::Dir<'static> = include_dir::Dir::new("", &[]);
/// What a binary that embeds nothing carries: no ids, no files, no default
/// deployer. moss-cli runs on this set and installs no bundled plugin.
pub static NO_BUNDLED_SET: BundledSet =
    BundledSet { names: &[], root: &NO_DIR, default_deployer: "" };

/// Seed the bundled set and return the one in force. First call wins, so a
/// later caller gets the set the process started with, never a second one.
pub fn seed_bundled_set(set: BundledSet) -> &'static BundledSet {
    BUNDLED_SET.get_or_init(|| set)
}

/// The bundled set in force: the app's embed once it has seeded, otherwise
/// [`NO_BUNDLED_SET`].
pub fn bundled_set() -> &'static BundledSet {
    BUNDLED_SET.get().unwrap_or(&NO_BUNDLED_SET)
}

fn default_deployer() -> &'static str {
    bundled_set().default_deployer
}


/// Read `.moss/config.toml` into a `toml::Value`, running migrations and
/// migrating IN MEMORY when the schema is old. Returns `Ok(None)` if the
/// file does not exist.
///
/// Since the M6a move this reader never writes: the on-disk migration (backup,
/// save, gitignore rewrite) is the app's job, run once per build/session via
/// `HostStore::run_vault_migrations` (ADR-059 — readers cross, writers stay).
/// Migrating the parsed value here keeps every read correct in the window
/// before that runner has persisted.
fn read_and_migrate_raw(project_path: &str) -> Result<Option<toml::Table>, String> {
    let config_path = std::path::Path::new(project_path).join(".moss").join("config.toml");
    let Some(original) = crate::build::site_config::read_managed_toml(&config_path)? else {
        return Ok(None);
    };
    let mut raw: toml::Table = toml::from_str(&original)
        .map_err(|e| format!("Failed to parse config.toml: {}", e))?;
    crate::config::migrations::migrate_to_current(&mut raw)
        .map_err(|e| format!("config migration failed: {e}"))?;
    Ok(Some(raw))
}

/// Read the set of installed channels from `.moss/config.toml`.
///
/// Channels are recorded as `[channels.<id>]` tables. Presence = installed.
/// A legacy config (which still carried `[hooks].syndicate`) is migrated in
/// memory for this read; the app persists the migration separately.
///
/// See `docs/reference/channels.md`.
pub fn get_channels_config(project_path: &str) -> Result<ChannelsConfig, String> {
    let Some(raw) = read_and_migrate_raw(project_path)? else {
        return Ok(ChannelsConfig::default());
    };
    let mut channels = BTreeMap::new();
    if let Some(channels_table) = raw.get("channels").and_then(|v| v.as_table()) {
        for (id, value) in channels_table {
            let table = value.as_table().cloned().unwrap_or_default();
            channels.insert(id.clone(), table);
        }
    }
    Ok(ChannelsConfig { channels })
}
/// Everything `discover_plugins` does except installing bundled plugins first.
///
/// Split out because "which plugins are installed" is a question a *read* is
/// allowed to ask. `discover_plugins` answers it by first extracting embedded
/// plugin code over `.moss/plugins/`, so any read routed through it silently
/// rewrites the user's installed plugins — which is how resolving a video size
/// cap came to overwrite `main.bundle.js` on builds that never asked for a
/// plugin at all.
pub fn discover_installed_plugins(project_path: &str) -> Result<Vec<Plugin>, String> {
    let plugins_dir = Path::new(project_path).join(".moss").join("plugins");

    // If plugins directory doesn't exist, return empty list (not an error)
    if !plugins_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plugins = Vec::new();

    // Scan plugins directory for subdirectories
    let entries = fs::read_dir(&plugins_dir)
        .map_err(|e| format!("Failed to read plugins directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read plugin entry: {}", e))?;
        let path = entry.path();

        // Only process directories
        if !path.is_dir() {
            continue;
        }

        // Try to load plugin from this directory
        match load_plugin(&path) {
            Ok(plugin) => plugins.push(plugin),
            Err(e) => {
                // Counted as well as logged: a plugin that does not load means
                // its content is missing from this build, so `--strict` must
                // fail on it and the CLI must print it — a consent refusal
                // that only reached the log file exited 0 with a page missing
                // (ADR-077). Other plugins still load.
                crate::build::cli_output::log_warn_problem!(target: "plugin", "⚠️ Plugin at {:?} will not run: {}", path, e);
            }
        }
    }

    Ok(plugins)
}

/// Load a single plugin from a directory
///
/// # Arguments
/// * `plugin_dir` - Absolute path to plugin directory
///
/// # Returns
/// * `Ok(Plugin)` - Loaded plugin
/// * `Err(String)` - Error message if loading fails
pub fn load_plugin(plugin_dir: &Path) -> Result<Plugin, String> {
    let manifest_path = plugin_dir.join("manifest.json");

    // Check if manifest.json exists
    if !manifest_path.exists() {
        return Err("manifest.json not found".to_string());
    }

    // Read and parse manifest
    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Failed to read manifest.json: {}", e))?;

    let manifest: PluginManifest = PluginManifest::parse(&manifest_content)
        .map_err(|e| format!("Failed to parse manifest.json: {}", e))?;

    // Whether this plugin may run at all is the app's call, not the build
    // tree's, and it is asked before anything else touches the plugin. Here
    // rather than in `discover_installed_plugins` because four of this
    // function's callers never go through that list — see `super::admission`.
    if let Some(refusal) = super::admission::refusal(&manifest, plugin_dir) {
        return Err(refusal);
    }

    // Validate entry point exists
    let entry_path = plugin_dir.join(&manifest.entry);
    if !entry_path.exists() {
        return Err(format!("Entry point '{}' not found", manifest.entry));
    }

    // Plugins are always active once discovered
    // Whether a plugin is actually used is determined by hooks config or auto-activation
    Ok(Plugin {
        manifest,
        path: plugin_dir.to_path_buf(),
    })
}

/// Load a single installed plugin by name, if it is installed.
///
/// Same loader `discover_plugins` uses, minus its `ensure_bundled_plugins_installed`
/// step — so this performs no writes. Use it for read-only lookups of a named
/// plugin's manifest (declared config defaults, capabilities); a read must not
/// install or update plugins as a side effect, least of all one running off a
/// background worker thread.
///
/// Returns `None` when the plugin is not installed or its directory is unusable.
pub fn load_installed_plugin(project_path: &str, plugin_name: &str) -> Option<Plugin> {
    let plugin_dir = Path::new(project_path)
        .join(".moss")
        .join("plugins")
        .join(plugin_name);
    load_plugin(&plugin_dir).ok()
}

/// Get plugin configuration from plugin folder or .moss/config.toml (fallback)
///
/// Configuration is read from these locations in order of precedence:
/// 1. `.moss/plugins/{plugin_name}/config.json` and `config.toml` (NEW: plugin
///    folder) — both are read and MERGED when present, since they're two
///    different writers (a plugin's own runtime state vs. the settings UI's
///    schema-driven fields), not alternatives. `config.json` wins on a
///    genuine key conflict.
/// 2. `.moss/config.toml [plugins.{plugin_name}]` section (OLD: global config,
///    for backwards compat) — only consulted when NEITHER plugin-folder file
///    exists.
///
/// # Arguments
/// * `project_path` - Absolute path to the project folder
/// * `plugin_name` - Name of the plugin
///
/// # Returns
/// * `Ok(HashMap)` - Plugin configuration key-value pairs
/// * `Err(String)` - Error message if reading fails
pub fn get_plugin_config(
    project_path: &str,
    plugin_name: &str,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    use std::collections::HashMap;

    let plugin_dir = Path::new(project_path)
        .join(".moss")
        .join("plugins")
        .join(plugin_name);

    // Plugin folder config: config.json (plugin runtime state — login binding,
    // last-sync bookkeeping, ...) and config.toml (schema-driven settings UI
    // fields) are two DIFFERENT writers into the same plugin folder, not
    // alternatives — a plugin's own saveConfig() calls can create config.json
    // at any time (e.g. on first login) independently of whatever the user
    // has toggled in Settings. Merge both when present so neither shadows the
    // other; config.json wins on a genuine key conflict (it's the more
    // specific, more recently-written runtime source).
    let plugin_config_json = plugin_dir.join("config.json");
    let plugin_config_toml = plugin_dir.join("config.toml");
    let json_map = plugin_config_json
        .exists()
        .then(|| read_plugin_config_json(&plugin_config_json))
        .transpose()?;
    let toml_map = plugin_config_toml
        .exists()
        .then(|| read_plugin_config_file(&plugin_config_toml))
        .transpose()?;

    match (json_map, toml_map) {
        (Some(mut json), Some(toml)) => {
            for (key, value) in toml {
                json.entry(key).or_insert(value);
            }
            return Ok(json);
        }
        (Some(json), None) => return Ok(json),
        (None, Some(toml)) => return Ok(toml),
        (None, None) => {}
    }

    // FALLBACK: the pre-config.json location, `[plugins.<name>]` in
    // .moss/config.toml — read through the one parse funnel
    // (`read_project_config` → `ConfigFile::parse`) so discovery sees
    // migrated values like every other config reader, instead of a raw
    // parse that would silently miss any future migration touching
    // `[plugins]`. A missing config is an empty config, not an error.
    let config = crate::build::site_config::read_project_config(project_path)?;
    if let Some(plugin_config) = config.section(&["plugins", plugin_name]) {
        // Convert TOML value to JSON value for easier handling
        let json_str = serde_json::to_string(plugin_config)
            .map_err(|e| format!("Failed to convert config to JSON: {}", e))?;
        let json_value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse config JSON: {}", e))?;

        if let serde_json::Value::Object(map) = json_value {
            return Ok(map.into_iter().collect());
        }
    }

    Ok(HashMap::new())
}

/// Hook-context config: persisted values overlaid on the manifest's declared
/// defaults (`PluginManifest::config_defaults`), so a field the user never persisted still
/// reaches hooks with its declared default — the same seeding rule the
/// settings UI applies in `plugin_config.rs`. Precedence: `config.json` >
/// `config.toml` > manifest defaults; an explicit persisted value (including
/// a falsy one) always wins over a default.
pub fn get_plugin_config_with_defaults(
    project_path: &str,
    plugin_name: &str,
    defaults: &std::collections::HashMap<String, serde_json::Value>,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut merged = defaults.clone();
    merged.extend(get_plugin_config(project_path, plugin_name).unwrap_or_default());
    merged
}

/// [`get_plugin_config_with_defaults`] for call sites that only hold the
/// plugin *name* plus the discovered plugin set: resolves the named plugin's
/// manifest defaults from `plugins` (unknown name ⇒ no defaults).
pub fn get_plugin_config_for_hook(
    project_path: &str,
    plugin_name: &str,
    plugins: &[super::types::Plugin],
) -> std::collections::HashMap<String, serde_json::Value> {
    let defaults = plugins
        .iter()
        .find(|p| p.manifest.name == plugin_name)
        .map(|p| p.manifest.config_defaults())
        .unwrap_or_default();
    get_plugin_config_with_defaults(project_path, plugin_name, &defaults)
}

/// Read plugin config from a JSON file in the plugin folder
fn read_plugin_config_json(
    config_path: &Path,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    use std::collections::HashMap;

    let content = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read plugin config.json: {}", e))?;

    let json_value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse plugin config.json: {}", e))?;

    if let serde_json::Value::Object(map) = json_value {
        Ok(map.into_iter().collect())
    } else {
        Ok(HashMap::new())
    }
}

/// Read plugin config from a standalone TOML file in the plugin folder
fn read_plugin_config_file(
    config_path: &Path,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    use std::collections::HashMap;

    let config_content = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read plugin config: {}", e))?;

    let config: toml::Value = toml::from_str(&config_content)
        .map_err(|e| format!("Failed to parse plugin config: {}", e))?;

    // The plugin folder config is a flat TOML file, not nested under [plugins.name]
    if let toml::Value::Table(table) = config {
        let mut result = HashMap::new();
        for (key, value) in table {
            // Convert TOML value to JSON value
            let json_str = serde_json::to_string(&value)
                .map_err(|e| format!("Failed to convert config value to JSON: {}", e))?;
            let json_value: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| format!("Failed to parse config JSON value: {}", e))?;
            result.insert(key, json_value);
        }
        return Ok(result);
    }

    Ok(HashMap::new())
}

/// Get hook configuration from .moss/config.toml
///
/// Reads the [hooks] section from config to determine which plugins handle which hooks
///
/// # Arguments
/// * `project_path` - Absolute path to the project folder
///
/// # Returns
/// * `Ok(HookConfig)` - Hook configuration with plugin assignments
/// * `Err(String)` - Error message if reading fails
pub fn get_hook_config(project_path: &str) -> Result<HookConfig, String> {
    let default = HookConfig {
        deploy: if default_deployer().is_empty() {
            None
        } else {
            Some(default_deployer().to_string())
        },
        ..HookConfig::default()
    };

    let Some(config) = read_and_migrate_raw(project_path)? else {
        return Ok(default);
    };

    let mut hook_config = default;

    // Extract hooks configuration (may override defaults)
    // Uses capability names: process, generate, enhance, deploy.
    // Channels are no longer in [hooks] — see get_channels_config.
    if let Some(hooks_table) = config.get("hooks").and_then(|v| v.as_table()) {
        let parse_array = |key: &str| -> Vec<String> {
            hooks_table.get(key)
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default()
        };
        let parse_single = |key: &str| -> Option<String> {
            hooks_table.get(key).and_then(|v| v.as_str().map(|s| s.to_string()))
        };

        hook_config.process = parse_array("process");
        // `[hooks] generate` / `[hooks] enhance` are no longer read (ADR-055).
        // An old config carrying either still parses — unknown keys in this
        // table have always been ignored — the key is simply inert.
        if let Some(deploy) = parse_single("deploy") {
            hook_config.deploy = Some(deploy);
        }
        // [hooks].syndicate is gone post-migration — channels live in [channels.*].
    }

    Ok(hook_config)
}

/// Which of these plugins publishes this site: the configured one, or the sole
/// deploy-capable one.
///
/// The policy, once, for the callers that already hold the plugin list:
/// `bundled::get_deploy_plugin` and the setup gate, which needs the `Plugin`
/// itself and not just its name.
///
/// It is deliberately narrower than "where does this site publish" — the sole
/// installed deploy plugin wins here with nothing configured. That fallback is
/// why nothing on the publish path may ask this directly; ask
/// [`crate::build::site_config::current_deploy_plugin`], which is what the Host
/// row shows the user.
pub fn resolve_deploy_plugin(
    project_path: &str,
    plugins: &[Plugin],
) -> Result<Option<String>, String> {
    let hook_config = get_hook_config(project_path).unwrap_or_default();
    if hook_config.deploy.is_some() {
        return Ok(hook_config.deploy);
    }
    resolve_sole_deploy_plugin(plugins)
}

/// Pick the one deploy-capable plugin, or say why that is not possible.
pub fn resolve_sole_deploy_plugin(plugins: &[Plugin]) -> Result<Option<String>, String> {
    let deploy_plugins: Vec<_> = plugins
        .iter()
        .filter(|p| p.manifest.has_capability(&Capability::Deploy))
        .collect();

    match deploy_plugins.len() {
        0 => Ok(None),
        1 => Ok(Some(deploy_plugins[0].manifest.name.clone())),
        _ => Err(format!(
            "Multiple deploy plugins found: {}. Configure [hooks] deploy = \"plugin-name\" in .moss/config.toml",
            deploy_plugins
                .iter()
                .map(|p| p.manifest.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}


/// Hook configuration from .moss/config.toml
///
/// Uses capability names that match plugin capabilities:
/// - process: Pre-process content before build (multiple plugins)
/// - generate: Build/generate the site (single plugin)
/// - enhance: Provide content for template slots (multiple plugins)
/// - deploy: Deploy to hosting platform (single plugin)
///
/// Channels (what used to live as `hooks.syndicate`) are recorded separately
/// under `[channels.<id>]` tables. See `ChannelsConfig` and
/// `docs/reference/channels.md`.
#[derive(Debug, Clone, Default)]
pub struct HookConfig {
    /// Plugins for process hook (array) - pre-process content before build
    pub process: Vec<String>,

    /// Plugin for deploy hook (single) - deploy to hosting platform
    pub deploy: Option<String>,
}

/// Check whether a login-capable plugin needs connection for the given project.
///
/// Returns `true` when ALL of:
///   - The plugin's `config.json` exists under `.moss/plugins/<plugin_id>/`
///   - `boundUserName` is absent or empty
///   - `login_dismissed` is not `true`
///
/// This is the signal the manager's process run reports as
/// `PipelineEvent::PluginNeedsConnection` before the hooks execute.
pub fn plugin_needs_connection(project_path: &str, plugin_id: &str) -> bool {
    let dismissed = get_plugin_config(project_path, plugin_id)
        .ok()
        .and_then(|c| c.get("login_dismissed").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    !plugin_is_bound(project_path, plugin_id) && !dismissed
}

/// Whether the plugin's per-folder config records a bound account
/// (`boundUserName` present and non-empty).
///
/// `affirmBindingFromProfile` writes `boundUserName` ONLY on a genuine successful
/// login — so this cleanly distinguishes a real login from a dismissed panel,
/// unlike [`plugin_needs_connection`] which returns `false` for a dismiss too.
/// Used by `connect_account` to fire the post-login process hook exactly once,
/// only when login actually bound a previously-unbound folder.
pub fn plugin_is_bound(project_path: &str, plugin_id: &str) -> bool {
    get_plugin_config(project_path, plugin_id)
        .ok()
        .and_then(|c| {
            c.get("boundUserName")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
        })
        .unwrap_or(false)
}

// The `[hooks]` writer, `save_hook_config`, lives in `domain::config`
// (ADR-059: readers travel with the pipeline into `crates/moss-build`;
// config.toml writers stay app-side).

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Hook contexts must see manifest-declared defaults for fields the user
    /// never persisted. The CLI runs with no settings ever saved, so without
    /// this overlay `context.config` is empty on a fresh project.
    #[test]
    fn test_config_with_defaults_unset_fields_use_manifest_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let mut defaults = std::collections::HashMap::new();
        defaults.insert("gateway".to_string(), serde_json::json!("https://default.example"));
        defaults.insert("retries".to_string(), serde_json::json!(3));

        let config = get_plugin_config_with_defaults(project_path, "ipfs", &defaults);

        assert_eq!(config.get("gateway"), Some(&serde_json::json!("https://default.example")));
        assert_eq!(config.get("retries"), Some(&serde_json::json!(3)));
    }

    /// Persisted values win over defaults — including a falsy value, which
    /// must never be resurrected to the truthy default — while untouched
    /// defaults still come through. Precedence: config.json > config.toml
    /// > manifest defaults.
    #[test]
    fn test_config_with_defaults_persisted_values_override_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let plugin_dir = temp_dir.path().join(".moss").join("plugins").join("ipfs");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("config.toml"), "enabled = false\n").unwrap();
        fs::write(
            plugin_dir.join("config.json"),
            r#"{"gateway": "https://user.example"}"#,
        )
        .unwrap();

        let mut defaults = std::collections::HashMap::new();
        defaults.insert("gateway".to_string(), serde_json::json!("https://default.example"));
        defaults.insert("enabled".to_string(), serde_json::json!(true));
        defaults.insert("retries".to_string(), serde_json::json!(3));

        let config = get_plugin_config_with_defaults(project_path, "ipfs", &defaults);

        assert_eq!(
            config.get("gateway"),
            Some(&serde_json::json!("https://user.example")),
            "config.json must win over the manifest default"
        );
        assert_eq!(
            config.get("enabled"),
            Some(&serde_json::json!(false)),
            "explicit false in config.toml must win over a true default"
        );
        assert_eq!(
            config.get("retries"),
            Some(&serde_json::json!(3)),
            "untouched default must still come through"
        );
    }

    fn plugin_with_config(
        name: &str,
        config: std::collections::HashMap<String, serde_json::Value>,
    ) -> crate::plugins::types::Plugin {
        crate::plugins::types::Plugin {
            manifest: crate::plugins::types::PluginManifest {
                name: name.into(),
                version: "0.0.1".into(),
                entry: "plugin.js".into(),
                config,
                ..Default::default()
            },
            path: std::path::PathBuf::from("/tmp/p"),
        }
    }

    /// Name-based variant: resolves the named plugin's manifest defaults from
    /// the discovered plugin set; an unknown name yields no defaults.
    #[test]
    fn test_config_for_hook_resolves_defaults_by_plugin_name() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let mut defaults = std::collections::HashMap::new();
        defaults.insert("gateway".to_string(), serde_json::json!("https://default.example"));
        let plugins = vec![plugin_with_config("ipfs", defaults)];

        let config = get_plugin_config_for_hook(project_path, "ipfs", &plugins);
        assert_eq!(
            config.get("gateway"),
            Some(&serde_json::json!("https://default.example"))
        );

        let other = get_plugin_config_for_hook(project_path, "unknown", &plugins);
        assert!(other.is_empty(), "unknown plugin name must yield no defaults");
    }


    #[test]
    fn test_load_plugin_missing_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("test-plugin");
        fs::create_dir(&plugin_dir).unwrap();

        let result = load_plugin(&plugin_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("manifest.json not found"));
    }

    #[test]
    fn test_load_plugin_valid_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("test-plugin");
        fs::create_dir(&plugin_dir).unwrap();

        // Create valid manifest
        let manifest = r#"{
            "name": "test-plugin",
            "version": "1.0.0",
            "entry": "main.js"
        }"#;
        fs::write(plugin_dir.join("manifest.json"), manifest).unwrap();

        // Create entry point
        fs::write(plugin_dir.join("main.js"), "// plugin code").unwrap();

        let result = load_plugin(&plugin_dir);
        assert!(result.is_ok());

        let plugin = result.unwrap();
        assert_eq!(plugin.manifest.name, "test-plugin");
        assert_eq!(plugin.manifest.version, "1.0.0");
    }

    #[test]
    fn test_get_plugin_config_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        let result = get_plugin_config(project_path, "test-plugin");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    // ============================================================================
    // Hook Configuration Tests
    // ============================================================================

    #[test]
    fn test_get_hook_config_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert!(config.process.is_empty());
    }

    #[test]
    fn test_get_hook_config_array_hooks() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Create config with array of hooks using capability names
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();

        // `enhance` is a retired key (ADR-055) and must not break the parse
        // of the table it sits in — an old config keeps working, inert.
        let config_content = r#"
[hooks]
process = ["plugin1", "plugin2", "plugin3"]
enhance = ["plugin4"]
        "#;
        fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.process, vec!["plugin1", "plugin2", "plugin3"]);
    }

    #[test]
    fn test_get_hook_config_single_value_hooks() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // `deploy` is the one single-value hook; `process` is the one array.
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();

        let config_content = r#"
[hooks]
deploy = "deploy-plugin"
        "#;
        fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.deploy, Some("deploy-plugin".to_string()));
    }

    #[test]
    fn test_get_hook_config_malformed_toml() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Create malformed TOML
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();

        let config_content = "this is [ not { valid toml";
        fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
    }

    #[test]
    fn test_get_hook_config_partial_configuration() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Only configure process, leave others empty
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();

        let config_content = r#"
[hooks]
process = ["test-plugin"]
        "#;
        fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.process, vec!["test-plugin"]);
        // deploy defaults to DEFAULT_DEPLOYER from build-time config (None if empty)
        if default_deployer().is_empty() {
            assert_eq!(config.deploy, None);
        } else {
            assert_eq!(config.deploy, Some(default_deployer().to_string()));
        }
    }

    // ============================================================================
    // Plugin Config from Plugin Folder Tests (NEW LOCATION)
    // ============================================================================

    #[test]
    fn test_get_plugin_config_from_plugin_folder() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Create plugin folder with config.toml
        let plugin_config_dir = temp_dir.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_config_dir).unwrap();

        let plugin_config_content = r#"
api_token = "secret123"
auto_sync = true
        "#;
        fs::write(plugin_config_dir.join("config.toml"), plugin_config_content).unwrap();

        let result = get_plugin_config(project_path, "test-plugin");
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.get("api_token").and_then(|v| v.as_str()), Some("secret123"));
        assert_eq!(config.get("auto_sync").and_then(|v| v.as_bool()), Some(true));
    }

    /// Regression test for the config.json/config.toml "split brain": settings
    /// schema fields (SchemaFieldRenderer) persist to config.toml, but a
    /// plugin's own runtime state (login binding, last-sync bookkeeping, ...)
    /// persists to config.json via its own storage helpers. Once config.json
    /// exists (created e.g. on first login), the OLD code returned early on
    /// the config.json branch and never looked at config.toml at all — so any
    /// settings toggle a user set would silently stop being seen by
    /// `context.config` at hook-execution time. Both plugin-folder sources
    /// must be merged, with config.json taking precedence on key conflicts
    /// (it's the more specific, more recently-written runtime source).
    #[test]
    fn test_get_plugin_config_merges_json_and_toml_in_plugin_folder() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        let plugin_dir = temp_dir.path().join(".moss").join("plugins").join("matters");
        fs::create_dir_all(&plugin_dir).unwrap();

        // config.toml: schema-driven settings fields (written by the Settings UI).
        fs::write(
            plugin_dir.join("config.toml"),
            r#"
add_canonical_link = true
sync_on_build = false
            "#,
        )
        .unwrap();

        // config.json: plugin runtime state (written by the plugin's own saveConfig()).
        fs::write(
            plugin_dir.join("config.json"),
            r#"{"boundUserName": "guo", "sync_on_build": true}"#,
        )
        .unwrap();

        let config = get_plugin_config(project_path, "matters").unwrap();

        // Keys unique to config.toml still surface even though config.json exists.
        assert_eq!(
            config.get("add_canonical_link").and_then(|v| v.as_bool()),
            Some(true),
            "config.toml-only key must not be shadowed by the presence of config.json"
        );
        // Keys unique to config.json are present too.
        assert_eq!(
            config.get("boundUserName").and_then(|v| v.as_str()),
            Some("guo")
        );
        // On a genuine key conflict, config.json (the more specific/recent
        // runtime source) wins over config.toml.
        assert_eq!(
            config.get("sync_on_build").and_then(|v| v.as_bool()),
            Some(true),
            "config.json must win over config.toml on a conflicting key"
        );
    }

    /// Isolates the (config.json exists, config.toml absent) arm of the merge
    /// — a plain passthrough, but every other arm has its own dedicated test.
    #[test]
    fn test_get_plugin_config_json_only_no_toml() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        let plugin_dir = temp_dir.path().join(".moss").join("plugins").join("matters");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("config.json"),
            r#"{"boundUserName": "guo", "lastSyncedAt": "2026-07-01T00:00:00.000Z"}"#,
        )
        .unwrap();

        let config = get_plugin_config(project_path, "matters").unwrap();
        assert_eq!(
            config.get("boundUserName").and_then(|v| v.as_str()),
            Some("guo")
        );
        assert_eq!(
            config.get("lastSyncedAt").and_then(|v| v.as_str()),
            Some("2026-07-01T00:00:00.000Z")
        );
    }

    #[test]
    fn test_plugin_folder_config_takes_precedence_over_global() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Create old location config in .moss/config.toml
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();
        let old_config = r#"
[plugins.test-plugin]
api_token = "old_token"
old_setting = true
        "#;
        fs::write(config_dir.join("config.toml"), old_config).unwrap();

        // Create new location config in .moss/plugins/test-plugin/config.toml
        let plugin_config_dir = config_dir.join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_config_dir).unwrap();
        let new_config = r#"
api_token = "new_token"
new_setting = true
        "#;
        fs::write(plugin_config_dir.join("config.toml"), new_config).unwrap();

        let result = get_plugin_config(project_path, "test-plugin");
        assert!(result.is_ok());

        let config = result.unwrap();
        // Plugin folder config should take precedence
        assert_eq!(config.get("api_token").and_then(|v| v.as_str()), Some("new_token"));
        assert_eq!(config.get("new_setting").and_then(|v| v.as_bool()), Some(true));
        // Old setting should NOT be present (plugin folder takes full precedence)
        assert!(config.get("old_setting").is_none());
    }

    #[test]
    fn test_get_plugin_config_falls_back_to_global_if_no_plugin_folder_config() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Only create old location config (no plugin folder config)
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();
        let old_config = r#"
[plugins.test-plugin]
api_token = "old_token"
        "#;
        fs::write(config_dir.join("config.toml"), old_config).unwrap();

        let result = get_plugin_config(project_path, "test-plugin");
        assert!(result.is_ok());

        let config = result.unwrap();
        // Should fall back to old location
        assert_eq!(config.get("api_token").and_then(|v| v.as_str()), Some("old_token"));
    }

    // ============================================================================
    // Hook Capability Name Tests
    // ============================================================================

    #[test]
    fn test_hook_config_reads_capability_names() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Create config with capability names
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();

        let config_content = r#"
[hooks]
process = ["preprocessor-plugin"]
generate = "generator-plugin"
enhance = ["enhancer-plugin"]
deploy = "deployer-plugin"

[channels.syndicator-plugin]
        "#;
        fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.process, vec!["preprocessor-plugin"]);
        assert_eq!(config.deploy, Some("deployer-plugin".to_string()));

        // Channels live in [channels.*] now — read via get_channels_config.
        let channels = get_channels_config(project_path).unwrap();
        assert!(channels.is_installed("syndicator-plugin"));
    }

    #[test]
    fn test_hook_config_multiple_plugins_per_hook() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        // Create config with multiple plugins for array hooks
        let config_dir = temp_dir.path().join(".moss");
        fs::create_dir_all(&config_dir).unwrap();

        let config_content = r#"
[hooks]
process = ["preprocessor1", "preprocessor2"]

[channels.matters]
[channels.twitter]
[channels.linkedin]
        "#;
        fs::write(config_dir.join("config.toml"), config_content).unwrap();

        let result = get_hook_config(project_path);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.process, vec!["preprocessor1", "preprocessor2"]);

        let channels = get_channels_config(project_path).unwrap();
        assert!(channels.is_installed("matters"));
        assert!(channels.is_installed("twitter"));
        assert!(channels.is_installed("linkedin"));
    }


}

#[cfg(test)]
mod channels_tests {
    use super::*;
    use tempfile::TempDir;

    /// Write a raw config.json for a plugin in a temp project.
    fn write_plugin_config(project: &TempDir, plugin_id: &str, json: &str) {
        let plugin_dir = project.path().join(".moss").join("plugins").join(plugin_id);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("config.json"), json).unwrap();
    }

    fn write_config(dir: &TempDir, contents: &str) -> String {
        let moss_dir = dir.path().join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        std::fs::write(moss_dir.join("config.toml"), contents).unwrap();
        dir.path().to_string_lossy().to_string()
    }

    #[test]
    fn legacy_hooks_syndicate_migrates_on_read() {
        let dir = TempDir::new().unwrap();
        let project = write_config(&dir, r#"
[hooks]
syndicate = ["email"]
"#);

        let channels = get_channels_config(&project).unwrap();
        assert!(channels.is_installed("email"));

        // In-memory only: the crate-side read never writes the vault. The
        // persist (rewrite + .bak-v0) is the app's, through the host seam
        // (ADR-059) — asserted in `infra/config_migrations_tests.rs`.
        let after = std::fs::read_to_string(dir.path().join(".moss/config.toml")).unwrap();
        assert!(after.contains("syndicate"), "after = {}", after);
        assert!(!dir.path().join(".moss/config.toml.bak-v0").exists());
    }

    #[test]
    fn channels_config_round_trips() {
        let dir = TempDir::new().unwrap();
        let project = write_config(&dir, &format!(r#"
schema_version = {}

[channels.email]
[channels.matters]
"#, crate::config::migrations::CURRENT_VERSION));

        let channels = get_channels_config(&project).unwrap();
        assert!(channels.is_installed("email"));
        assert!(channels.is_installed("matters"));
        assert!(!channels.is_installed("bluesky"));

        // No migration, no backup.
        assert!(!dir.path().join(".moss/config.toml.bak-v0").exists());
    }

    #[test]
    fn missing_config_returns_empty_channels() {
        let dir = TempDir::new().unwrap();
        let channels = get_channels_config(&dir.path().to_string_lossy()).unwrap();
        assert!(!channels.is_installed("email"));
    }

    /// Pre-check (2026-05-22 plan): refute the "nested-only TOML breaks
    /// email installation detection" hypothesis. A real imported site's
    /// config carries only `[channels.email.send_mode]` (no bare
    /// `[channels.email]` table). The parser must still surface `email` as
    /// installed; otherwise the syndicator-plugins backend silently drops
    /// the channel and the control-panel icon goes missing — see the
    /// "Unconfirmed open questions" section of
    /// `docs/archive/2026-05-22-email-channel-control-panel.md`.
    ///
    /// Locking this with a test (instead of a one-off REPL check) means a
    /// future refactor of `get_channels_config` cannot regress on the
    /// nested-table shape.
    #[test]
    fn nested_email_send_mode_table_marks_email_installed() {
        let dir = TempDir::new().unwrap();
        let toml = r#"schema_version = 3

[channels.email.send_mode]
mode = "all"

[site]
lang = "en"
"#;
        let project = write_config(&dir, toml);
        let channels = get_channels_config(&project).unwrap();
        assert!(
            channels.is_installed("email"),
            "is_installed(\"email\") must be true for nested [channels.email.send_mode] config — \
             this is the exact byte shape from a real site's .moss/config.toml; \
             a regression here means the control-panel icon goes missing."
        );
    }

    // ─── plugin_needs_connection ───────────────────────────────────────────────

    /// No config.json → no bound user → needs connection.
    #[test]
    fn test_needs_connection_no_config_file() {
        let dir = TempDir::new().unwrap();
        // No .moss/plugins/matters/ dir → get_plugin_config returns empty map.
        // plugin_needs_connection returns false because boundUserName is absent
        // AND no config at all means the plugin may not even be installed.
        // Correct: returns false (no config = plugin not yet installed/configured;
        // Rust can't meaningfully tell "needs connection" from "not installed").
        let result = plugin_needs_connection(
            dir.path().to_str().unwrap(),
            "matters",
        );
        // With no config.json, boundUserName is absent ↔ has_bound_user=false,
        // and dismissed=false (default) → returns true (needs connection).
        assert!(result, "No config at all → plugin_needs_connection should return true");
    }

    /// Config has boundUserName → does NOT need connection.
    #[test]
    fn test_needs_connection_with_bound_user() {
        let dir = TempDir::new().unwrap();
        write_plugin_config(&dir, "matters", r#"{"boundUserName":"@alice"}"#);
        assert!(
            !plugin_needs_connection(dir.path().to_str().unwrap(), "matters"),
            "boundUserName present → needs_connection should be false"
        );
    }

    /// login_dismissed=true → does NOT need connection even without boundUserName.
    #[test]
    fn test_needs_connection_dismissed_flag() {
        let dir = TempDir::new().unwrap();
        write_plugin_config(&dir, "matters", r#"{"login_dismissed":true}"#);
        assert!(
            !plugin_needs_connection(dir.path().to_str().unwrap(), "matters"),
            "login_dismissed=true → needs_connection should be false"
        );
    }

    /// No boundUserName + login_dismissed=false → needs connection.
    #[test]
    fn test_needs_connection_no_user_no_dismiss() {
        let dir = TempDir::new().unwrap();
        write_plugin_config(&dir, "matters", r#"{"login_dismissed":false}"#);
        assert!(
            plugin_needs_connection(dir.path().to_str().unwrap(), "matters"),
            "login_dismissed=false + no boundUserName → needs_connection should be true"
        );
    }

    // ─── plugin_is_bound + post-login-sync gate (download-after-login regression) ─

    /// No config → not bound.
    #[test]
    fn test_is_bound_no_config() {
        let dir = TempDir::new().unwrap();
        assert!(!plugin_is_bound(dir.path().to_str().unwrap(), "matters"));
    }

    /// boundUserName present + non-empty → bound (a real login succeeded).
    #[test]
    fn test_is_bound_with_user() {
        let dir = TempDir::new().unwrap();
        write_plugin_config(&dir, "matters", r#"{"boundUserName":"@alice"}"#);
        assert!(plugin_is_bound(dir.path().to_str().unwrap(), "matters"));
    }

    /// Empty boundUserName → not bound.
    #[test]
    fn test_is_bound_empty_user() {
        let dir = TempDir::new().unwrap();
        write_plugin_config(&dir, "matters", r#"{"boundUserName":""}"#);
        assert!(!plugin_is_bound(dir.path().to_str().unwrap(), "matters"));
    }

    /// CRITICAL: a DISMISSED login must NOT read as bound. `plugin_needs_connection`
    /// returns false for a dismiss (so it CANNOT distinguish dismiss from a real
    /// login) — which is exactly why `connect_account` gates the post-login process
    /// hook on `plugin_is_bound`, not `plugin_needs_connection`. A dismiss ⇒ no
    /// spurious download.
    #[test]
    fn test_is_bound_dismissed_is_not_bound() {
        let dir = TempDir::new().unwrap();
        write_plugin_config(&dir, "matters", r#"{"login_dismissed":true}"#);
        let p = dir.path().to_str().unwrap();
        assert!(!plugin_is_bound(p, "matters"), "dismiss must not read as bound");
        // The conflation the gate avoids: needs_connection is ALSO false for a
        // dismiss, so gating the post-login sync on it would wrongly fire.
        assert!(
            !plugin_needs_connection(p, "matters"),
            "dismiss ⇒ needs_connection is false too (the trap plugin_is_bound sidesteps)"
        );
    }

    /// The `connect_account` gate `!bound_before && bound_after` fires the
    /// download ONLY on a genuine login (unbound → bound), and is inert on a
    /// dismiss and on an already-bound folder (which the next build syncs).
    #[test]
    fn test_post_login_sync_gate_fires_only_on_real_login() {
        // Fresh unbound folder → real login binds it → gate fires.
        let dir = TempDir::new().unwrap();
        let p = dir.path().to_str().unwrap();
        write_plugin_config(&dir, "matters", r#"{}"#);
        let bound_before = plugin_is_bound(p, "matters");
        write_plugin_config(&dir, "matters", r#"{"boundUserName":"@alice"}"#);
        assert!(
            !bound_before && plugin_is_bound(p, "matters"),
            "unbound → bound must fire the post-login download"
        );

        // Already-bound re-login → gate inert (no spurious re-sync).
        let bound_before2 = plugin_is_bound(p, "matters");
        assert!(
            !(!bound_before2 && plugin_is_bound(p, "matters")),
            "already-bound re-login must NOT fire the download"
        );

        // Dismiss on a fresh folder → still unbound → gate inert.
        let dir2 = TempDir::new().unwrap();
        let p2 = dir2.path().to_str().unwrap();
        write_plugin_config(&dir2, "matters", r#"{}"#);
        let bb = plugin_is_bound(p2, "matters");
        write_plugin_config(&dir2, "matters", r#"{"login_dismissed":true}"#);
        assert!(
            !(!bb && plugin_is_bound(p2, "matters")),
            "dismissed login must NOT fire the download"
        );
    }
}
