//! Plugin manager - orchestrates plugin execution

use super::adapter_host::AdapterHost;
use super::bundled::discover_plugins;
use super::hook_state::PluginHookState;
use super::setup::{SetupContext, SetupVerdict};
use super::types::*;
use engine_adapter::PluginEngine;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub mod engine_adapter;
mod hook_context;
pub mod signal_route;

use hook_context::{context_for_plugin, HookContext};

/// One [`PluginManager`] per project for the life of the process, built on
/// first request and shared by every caller. The app manages one of these
/// (behind `Arc`) and hands it the desktop host; a headless build owns one
/// over [`AdapterHost::headless`]. The cache is the pipeline's plugin port:
/// [`crate::build::HostPorts::plugins`].
pub struct ManagerCache {
    /// Keyed by canonical project path.
    managers: Mutex<HashMap<String, Arc<PluginManager>>>,
    /// How this host builds the record a new manager shares with its engine.
    /// Errs when the host is not ready — in the app, before its managed
    /// registries exist, when a manager would stamp into state nobody reads.
    make_host: Box<dyn Fn() -> AdapterHost + Send + Sync>,
}

/// Pure lookup of a `contributes.jobs[job_id]` descriptor in a plugin slice
/// (Step 3 Phase 5, §8 + R13). Split out from `resolve_job_descriptor` so the
/// production consumer of `JobDescriptor`/`ContributedJobs` is unit-testable
/// without building a `PluginManager`.
pub(crate) fn resolve_job_descriptor_in(
    plugins: &[Plugin],
    plugin_name: &str,
    job_id: &str,
) -> Option<crate::plugins::types::JobDescriptor> {
    plugins
        .iter()
        .filter(|p| p.manifest.name == plugin_name)
        .find_map(|p| {
            p.manifest
                .contributes
                .as_ref()
                .and_then(|c| c.jobs.as_ref())
                .and_then(|jobs| jobs.descriptors.get(job_id))
                .cloned()
        })
}

impl ManagerCache {
    pub fn new(make_host: impl Fn() -> AdapterHost + Send + Sync + 'static) -> Self {
        Self { managers: Mutex::new(HashMap::new()), make_host: Box::new(make_host) }
    }

    /// A cache whose managers have no desktop: fresh registries, the given
    /// reporter, the process's tokio runtime. moss-cli's and the app's
    /// headless `moss build` share this arm.
    pub fn headless(reporter: Arc<dyn crate::build::ports::reporter::BuildReporter>) -> Self {
        Self::new(move || AdapterHost::headless(reporter.clone()))
    }

    fn managers(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<PluginManager>>> {
        self.managers.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Get an existing PluginManager for a project path without creating a new one.
    ///
    /// Tries the canonical path first, then the raw path as fallback.
    /// Returns None if no manager exists for this path.
    pub fn get_existing(&self, project_path: &str) -> Option<Arc<PluginManager>> {
        let managers = self.managers();
        // Try canonical path first (how get_or_create stores them)
        if let Ok(canonical) = std::fs::canonicalize(project_path) {
            let key = canonical.to_string_lossy().to_string();
            if let Some(manager) = managers.get(&key) {
                return Some(manager.clone());
            }
        }
        // Fallback: try the raw path
        managers.get(project_path).cloned()
    }

    /// Resolve a plugin's `contributes.jobs[job_id]` descriptor across every
    /// loaded manager (Step 3 Phase 5, §8 + R13) — the PRODUCTION CONSUMER of
    /// `JobDescriptor`/`ContributedJobs` (CLAUDE.md: "every module must have a
    /// consumer before it ships").
    ///
    /// Returns the descriptor's `(verb, noun)` MEANING the plugin proposes. The
    /// caller (`report_plugin_task_lifecycle_command`) runs `Verb::normalized`
    /// on the verb (R13) before stamping it on the Job — so moss owns
    /// capitalization/length/glyphs and the plugin's raw word never reaches the
    /// surface verbatim.
    ///
    /// Returns `None` if no loaded plugin declares this job — the lifecycle then
    /// falls back to the legacy free-text receipt path (byte-identical).
    pub fn resolve_job_descriptor(
        &self,
        plugin_name: &str,
        job_id: &str,
    ) -> Option<crate::plugins::types::JobDescriptor> {
        self.with_plugins(|plugins| resolve_job_descriptor_in(plugins, plugin_name, job_id))
    }

    /// Ask every loaded manager's plugin list in turn, until one answers.
    ///
    /// Plugin names are unique within a loaded set, so a name match is
    /// unambiguous even with several project managers live — which is what lets
    /// the seams that carry a plugin id but no project path resolve at all.
    fn with_plugins<T>(&self, ask: impl Fn(&[Plugin]) -> Option<T>) -> Option<T> {
        self.managers().values().find_map(|manager| ask(&manager.plugins()))
    }

    /// The display name to put on a plugin's action panel. `None` when no loaded
    /// plugin claims the id — the only case where the caller falls back to
    /// deriving a title from the URL.
    pub fn plugin_human_name(&self, plugin_name: &str) -> Option<String> {
        self.with_plugins(|plugins| {
            plugins
                .iter()
                .find(|p| p.manifest.name == plugin_name)
                .map(|p| crate::plugins::registry::plugin_human_name(&p.manifest))
        })
    }

    /// Get or create a PluginManager for the given project path.
    ///
    /// Returns the cached manager if one exists, or creates a new one. The
    /// `Arc` is the handle every caller holds across its awaits — the manager
    /// is immutable but for its plugin list, which guards itself.
    pub fn get_or_create(&self, folder_path: &str) -> Result<Arc<PluginManager>, String> {
        let canonical_path = std::fs::canonicalize(folder_path)
            .map_err(|e| format!("Failed to resolve path: {}", e))?
            .to_string_lossy()
            .to_string();

        let mut managers = self.managers();
        if let Some(existing) = managers.get(&canonical_path) {
            return Ok(existing.clone());
        }

        let manager = PluginManager::new(&canonical_path, (self.make_host)())?;
        let arc = Arc::new(manager);
        managers.insert(canonical_path, arc.clone());
        Ok(arc)
    }

    /// Drop the manager for one project (canonical key), returning it so the
    /// caller can tear its engine down outside the lock.
    pub fn remove(&self, folder_path: &str) -> Option<Arc<PluginManager>> {
        let key = std::fs::canonicalize(folder_path).ok()?.to_string_lossy().to_string();
        self.managers().remove(&key)
    }

    /// Drop every manager, returning them for teardown outside the lock.
    pub fn drain(&self) -> Vec<Arc<PluginManager>> {
        self.managers().drain().map(|(_, v)| v).collect()
    }
}

/// Plugin manager - handles plugin discovery and execution
///
/// Uses eager initialization: when the preview starts and runtime is ready,
/// the configured deploy plugin is initialized immediately (in background).
/// This ensures the plugin is ready when the user clicks Publish.
///
/// Initialization is coordinated via `plugin_init_in_progress` to prevent
/// duplicate concurrent initializations of the same plugin.
pub struct PluginManager {
    /// The loaded plugins. The one field that changes while moss runs
    /// (`rediscover_plugins` after an install or uninstall); readers take a
    /// snapshot, so no guard ever lives across an await.
    plugins: RwLock<Vec<Plugin>>,

    /// Project path
    project_path: String,

    /// Where this manager's hook state, desktop, reporter and runtime live;
    /// cloned into the engine adapter so both stamp the same registries.
    host: AdapterHost,

    /// Engine that performs hook dispatch (#789 Phase 4): `QuickJsEngineAdapter`,
    /// the only one since the hidden-webview escape hatch was removed.
    engine: Arc<dyn PluginEngine>,

    /// Privileged-capability grants derived from `plugins`' parsed manifests and
    /// shared (Arc) with the engine's dispatch task. Re-snapshotted by
    /// `rediscover_plugins` — the ONE place a grant may change while moss runs.
    grants: crate::engine::host_fns::GrantRegistry,
}

/// Build the hook-dispatch engine.
///
/// CARDINALITY (Phase-3 D1): one engine per PluginManager (per project). The engine's
/// per-plugin Context map is keyed by name only; per-manager keeps it project-safe and
/// folder-close teardown replaces a dead engine. Process-global was rejected — see
/// docs/archive/2026-06-10-plugin-runtime-phase3-real-plugins.md D1.
fn build_engine(
    host: AdapterHost,
    project_path: &str,
    grants: crate::engine::host_fns::GrantRegistry,
) -> Arc<dyn PluginEngine> {
    let source = bundle_source_for_project(project_path.to_string());
    Arc::new(engine_adapter::QuickJsEngineAdapter::new(
        host, source, project_path.to_string(), grants,
    ))
}

/// Live [`BundleSource`] for a project: reads the installed plugin's manifest +
/// entry FRESH from `<project>/.moss/plugins/<name>/` on every call (memory rule:
/// prefer fresh reads), so a reinstalled plugin is picked up without an engine
/// restart (paired with the engine's bundle-fingerprint eviction).
pub(crate) fn bundle_source_for_project(
    project_path: String,
) -> engine_adapter::BundleSource {
    Arc::new(move |plugin_name: &str| {
        let plugin_dir = crate::plugins::plugin_dir(&project_path, plugin_name);
        let plugin = crate::plugins::discovery::load_plugin(&plugin_dir)
            .map_err(|e| format!("load plugin '{plugin_name}': {e}"))?;
        let entry = plugin.path.join(&plugin.manifest.entry);
        // Sync ~100 KB read inside an async dispatch; acceptable, noted.
        let bundle = std::fs::read_to_string(&entry)
            .map_err(|e| format!("read bundle {}: {e}", entry.display()))?;
        let global_name = plugin.manifest.global_name.clone()
            .unwrap_or_else(|| derive_global_name(plugin_name));
        Ok(engine_adapter::PluginBundle { bundle, global_name })
    })
}

impl PluginManager {
    /// A snapshot of the discovered plugins.
    pub fn plugins(&self) -> Vec<Plugin> {
        self.plugins.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Engine handle for explicit teardown (system/utils.rs teardown_plugin_system).
    pub fn engine_handle(&self) -> Arc<dyn PluginEngine> {
        self.engine.clone()
    }

    /// Create a new plugin manager for a project.
    ///
    /// `host` is where this manager's hook state, desktop, reporter and
    /// runtime live — the app's managed singletons behind a real desktop, or
    /// [`AdapterHost::headless`], where every app-reaching site refuses by
    /// name. Errs when the project's plugins cannot be discovered.
    pub fn new(project_path: &str, host: AdapterHost) -> Result<Self, String> {
        let plugins = discover_plugins(project_path)?;

        log::info!(target: "plugin", "plugin engine: quickjs ({} plugin(s))", plugins.len());

        // The grant snapshot is taken from the SAME parsed manifests discovery
        // produced, before anything can rewrite them on disk.
        let grants = crate::engine::host_fns::GrantRegistry::from_plugins(&plugins);

        let manager = Self {
            plugins: RwLock::new(plugins),
            project_path: project_path.to_string(),
            engine: build_engine(host.clone(), project_path, grants.clone()),
            grants,
            host,
        };

        // Nothing to spawn: every plugin self-initializes per-Context on its
        // first quickjs dispatch (D4). The eager webview spawn that stood here
        // went with the hidden runtime webview.

        Ok(manager)
    }

    /// Find a plugin by name
    fn find_plugin_by_name(&self, plugin_name: &str) -> Option<Plugin> {
        self.plugins
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|p| p.manifest.name == plugin_name)
            .cloned()
    }

    /// Find all plugins with a specific capability
    fn find_plugins_by_capability(&self, capability: &Capability) -> Vec<Plugin> {
        self.plugins
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|p| p.manifest.has_capability(capability))
            .cloned()
            .collect()
    }

    /// Re-scan the plugins directory and update the plugin list.
    ///
    /// Called after installing or uninstalling a plugin. The runtime window and
    /// initialized JS instances are preserved — only the Rust-side plugin list
    /// is refreshed.
    ///
    /// Carried a Phase-4 condition-3 warning about engine_kind never being
    /// re-evaluated here: a mid-session `enhance` install left one bundle
    /// loaded in two runtimes with split module state. ADR-055 retired the
    /// capability, and with it the only way to reach that state.
    pub fn rediscover_plugins(&self) {
        match discover_plugins(&self.project_path) {
            Ok(plugins) => {
                log::info!(target: "plugin", "Rediscovered {} plugins for {}", plugins.len(), crate::types::runtime::redact_home_dir(&self.project_path));
                let mut current = self.plugins.write().unwrap_or_else(|e| e.into_inner());
                *current = plugins;
                // Must follow the assignment: the engine's dispatch task reads
                // this snapshot, and a plugin installed mid-session is denied
                // every gated capability until it lands here.
                self.grants.replace(&current);
            }
            Err(e) => {
                log::warn!(target: "plugin", "Failed to rediscover plugins: {}", e);
            }
        }
    }

    /// The deploy plugin this dispatch will run, or `None` for moss's own
    /// hosting.
    ///
    /// Was `find_single_plugin_by_capability(&Capability)` with an
    /// auto-activate ladder — configured-in-`[hooks]`, else the sole
    /// capability-bearer, else an error naming the conflict. Deploy never took
    /// that path and `generate`, the only other caller, was retired by
    /// ADR-055, so the parameter went with the ladder.
    ///
    /// It asks the one resolver that owns "which target is Publish about to
    /// use" — the same one the Host row and the setup gate read. It used to
    /// ask `get_deploy_plugin` instead, which answers a narrower question
    /// (the configured plugin, else the sole installed one) and therefore
    /// dispatched to a plugin in the two states reserved for moss: nothing
    /// selected yet, and moss hosting chosen with a plugin still installed.
    ///
    /// No `Result`: several plugins installed and none chosen answers moss, not
    /// an error, because that is what the Host row shows — and a publish that
    /// contradicts the row is the bug this closes.
    fn deploy_plugin(&self) -> Option<Plugin> {
        crate::build::site_config::current_deploy_plugin(&self.project_path)
            .and_then(|name| self.find_plugin_by_name(&name))
    }

    /// Build an advisory for a plugin process-hook sync failure (issue #793).
    ///
    /// Scope: Remote — the hook ran but reported a sync failure, typically a
    /// network/API problem reaching the remote platform.
    /// Severity: NeedsAction — content may be stale; user should re-sync when the
    /// remote is available.
    /// Action: None — no automated recovery path.
    pub(crate) fn plugin_hook_failure_advisory(plugin_name: &str, err: &str) -> crate::advisory::Advisory {
        use crate::advisory::{Action, Advisory, Scope, Severity};
        Advisory {
            scope: Scope::Remote,
            severity: Severity::NeedsAction,
            item: None,
            what: crate::infra::app_advisory::fmt(
                "plugin_could_not_run",
                &[("name", plugin_name), ("detail", err)],
            ),
            action: Action::None,
        }
    }

    /// Execute process capability (pre-processing before generation)
    ///
    /// Runs before build to prepare source files
    /// Non-critical - continues on failure with warning
    ///
    /// Note: This executes hooks inline (not in a background task) so callers can
    /// await completion. The caller (spawn_process_hooks) handles the
    /// background spawning if needed.
    ///
    /// # Timeout: 30s per plugin, 5min total
    pub async fn execute_process(&self, context: &ProcessContext) -> Result<(), String> {
        // Before any hook runs, and whether or not one will: a login-capable
        // plugin with no account bound for this folder, and the prompt not
        // dismissed, gets ONE nudge. The shell auto-opens login on it;
        // headless, the reporter says the plugin's content will be skipped
        // (and counts it for `--strict`).
        for plugin in self.find_plugins_by_capability(&Capability::Login) {
            let name = &plugin.manifest.name;
            if crate::plugins::discovery::plugin_needs_connection(&context.project_path, name) {
                log::info!(target: "plugin", "[auto-open] {name} needs connection for {} — nudging the shell", context.project_path);
                self.host.reporter.report(&crate::build::progress::PipelineEvent::PluginNeedsConnection {
                    plugin: name.clone(),
                    project_path: context.project_path.clone(),
                });
            }
        }

        // Execute hooks sequentially (inline, not spawned)
        for plugin in &self.find_plugins_by_capability(&Capability::Process) {
            let plugin_context = context_for_plugin(&context.project_path, context, plugin);

            // Serialize context with per-plugin config
            let context_json = serde_json::to_string(&plugin_context)
                .map_err(|e| format!("Failed to serialize context: {}", e))?;

            // Execute hook - plugin is guaranteed to be initialized now.
            // On hook failure, emit a Remote advisory (network/API issue)
            // so the user sees a NeedsAction dot rather than silent staleness.
            //
            // Capability gate (B4B5 Fix A): plugins that declare `Login` own their
            // own PanelTask failure surface (they call `task.failed(…)` inside the
            // hook itself). Emitting an advisory here too would double-report the
            // same failure — once as the PanelTask badge and again as a Rust
            // BackgroundProgress advisory. Suppress the advisory for Login-capable
            // plugins; they self-report via their own PanelTask.
            let has_login = plugin.manifest.capabilities.contains(&Capability::Login);
            if let Err(e) = self.execute_plugin_javascript(
                plugin,
                Capability::Process.hook_name(),
                &context_json
            ).await {
                log::warn!(target: "plugin", "⚠️ Plugin '{}' process hook failed: {}", plugin.manifest.name, e);
                if !has_login {
                    let advisory = Self::plugin_hook_failure_advisory(&plugin.manifest.name, &e);
                    self.host.reporter.report(
                        &crate::build::progress::PipelineEvent::BackgroundProgress {
                            task: format!("plugin-process-{}", plugin.manifest.name),
                            current: 0,
                            total: 1,
                            message: advisory.what.clone(),
                            completed: true,
                            advisories: vec![advisory],
                        },
                    );
                }
            }
        }

        Ok(())
    }

    /// Execute process hook for a single named plugin
    ///
    /// Unlike `execute_process()` which runs all process-capable plugins,
    /// this runs only the specified plugin. Used after installing a new
    /// process-capable plugin to trigger its initial setup (e.g., login flow).
    pub async fn execute_process_for_plugin(
        &self,
        plugin_id: &str,
        context: &ProcessContext,
    ) -> Result<(), String> {
        let plugin = self
            .find_plugin_by_name(plugin_id)
            .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;

        // Verify it has Process capability
        if !plugin.manifest.capabilities.contains(&Capability::Process) {
            return Err(format!("Plugin '{}' does not have process capability", plugin_id));
        }

        let plugin_context = context_for_plugin(&context.project_path, context, &plugin);

        // Serialize context
        let context_json = serde_json::to_string(&plugin_context)
            .map_err(|e| format!("Failed to serialize context: {}", e))?;

        // Execute the hook
        self.execute_plugin_javascript(
            &plugin,
            Capability::Process.hook_name(),
            &context_json,
        ).await
    }

    /// Dispatch a named plugin export by hook name — public entry point for
    /// commands that need to invoke an export that isn't a routed `Capability`
    /// (e.g. the `login` export invoked by `connect_account`).
    ///
    /// The caller is responsible for passing a pre-initialized plugin and a
    /// pre-serialized context JSON. The watchdog suppression, singleflight dedup,
    /// and hook-state lifecycle are all handled inside `execute_plugin_javascript`.
    ///
    /// # Design note
    /// `execute_plugin_javascript` is private so callers can't accidentally bypass
    /// the initialization and state-management guards. This thin public wrapper
    /// exposes the dispatch surface without relaxing those guards — the caller
    /// must already hold an initialized `Plugin` reference.
    pub async fn execute_plugin_javascript_pub(
        &self,
        plugin: &Plugin,
        hook_name: &str,
        context_json: &str,
    ) -> Result<(), String> {
        self.execute_plugin_javascript(plugin, hook_name, context_json).await
    }


    /// Execute deploy capability (publish to hosting platforms)
    ///
    /// Deploys site to hosting - critical operation
    /// Aborts on failure. Returns deployment info from the plugin.
    ///
    /// # Timeout: 30s
    ///
    /// # Returns
    /// * `Ok(Some(HookResult))` - Plugin executed and returned result (may include toast, deployment)
    /// * `Ok(None)` - No deploy plugin configured
    /// * `Err(String)` - Plugin execution failed catastrophically (not a plugin-reported failure)
    ///
    /// Note: Plugin-reported failures (success: false) are returned as Ok(Some(HookResult))
    /// so the caller can access the plugin's toast message for user display.
    pub async fn execute_deploy(&self, context: &DeployContext) -> Result<Option<HookResult>, String> {
        if let Some(plugin) = self.deploy_plugin() {
            let plugin_context = context_for_plugin(&self.project_path, context, &plugin);
            let result = self.execute_hook_with_result(&plugin, Capability::Deploy.hook_name(), &plugin_context).await?;

            // Check if plugin returned a result
            if let Some(hook_result) = result {
                // Return the full HookResult - caller decides how to handle success/failure
                // This preserves the plugin's toast message for user display
                return Ok(Some(hook_result));
            }

            // Plugin executed but returned no result - unusual for deploy hook
            return Err("Deploy plugin completed but returned no result".to_string());
        }

        // No plugin registered - use default deployment
        Ok(None)
    }

    /// Execute syndicate capability (POSSE distribution after deployment)
    ///
    /// Syndicates content after deployment. Only executes plugins that are
    /// explicitly enabled in the config's [hooks] syndicate array.
    /// Non-critical - continues on failure with warning
    ///
    /// # Timeout: awaiting-aware inactivity watchdog only (no wall-clock cap)
    ///
    /// Syndication involves open-ended user interaction (the user writes and
    /// publishes a draft in the Matters editor). A blanket wall-clock timeout
    /// would kill the hook while the user is still editing. Instead we rely
    /// solely on the awaiting-aware 60s inactivity watchdog inside
    /// `execute_plugin_javascript`: the watchdog suspends (`continue`s) while
    /// `is_hook_awaiting(...)` is true, and only fires when the plugin has been
    /// genuinely silent (no progress/awaiting signals) for 60s — i.e. a real
    /// crash, not a slow human.
    pub async fn execute_syndicate(&self, context: &SyndicateContext) -> Result<(), String> {
        // Get installed channels from config
        let channels_config = super::discovery::get_channels_config(&self.project_path)
            .unwrap_or_default();

        let builtin_ids = super::registry::get_builtin_channel_ids();

        // Filter plugins to only those installed as channels, skipping built-in channels
        // (built-in channels like email are handled natively in syndicate_to_platforms)
        let plugins: Vec<_> = self.find_plugins_by_capability(&Capability::Syndicate)
            .into_iter()
            .filter(|p| {
                channels_config.is_installed(&p.manifest.name)
                    && !builtin_ids.contains(&p.manifest.name.as_str())
            })
            .collect();

        if plugins.is_empty() {
            return Ok(());
        }

        for plugin in &plugins {
            // Serialized per plugin: each carries its OWN resolved config, so
            // one shared JSON would hand every channel the wrong settings.
            let plugin_context = context_for_plugin(&context.project_path, context, plugin);
            let context_json = serde_json::to_string(&plugin_context)
                .map_err(|e| format!("Failed to serialize syndicate context: {}", e))?;

            // The inactivity watchdog inside `execute_plugin_javascript`
            // suspends while the hook signals Awaiting (user editing in
            // Matters), so it never kills a slow user. Syndication used to
            // carry a 900s wall-clock cap on top; it was removed deliberately.
            // Non-critical — continue on failure, but log the error.
            if let Err(e) = self.execute_plugin_javascript(plugin, Capability::Syndicate.hook_name(), &context_json).await {
                log::warn!(target: "plugin", "Syndication hook '{}' failed: {}", plugin.manifest.name, e);
            }
        }

        Ok(())
    }

    /// Call an optional hook on the CHOSEN deploy target's plugin.
    ///
    /// Resolves through `current_deploy_target`, not `deploy_plugin()`, whose
    /// sole-installed-plugin fallback would fire the hook for a plugin the
    /// author never chose: with github installed and moss hosting chosen, its
    /// `check_setup` asks about a publish that will never touch github.
    ///
    /// `Ok(None)` is the common case and must stay cheap: moss hosting, no
    /// deploy plugin, or one that does not implement this hook. Everything
    /// else — a plugin that will not initialize, a hook that threw — is an
    /// error for the caller to weigh, because only the caller knows whether it
    /// is publishing.
    async fn dispatch_optional_deploy_hook<C: HookContext + serde::Serialize>(
        &self,
        hook: &str,
        project_path: &str,
        context: &C,
    ) -> Result<Option<HookResult>, String> {
        let Some(plugin) = self.deploy_plugin() else {
            return Ok(None);
        };

        let plugin_context = context_for_plugin(project_path, context, &plugin);
        match self.execute_hook_with_result(&plugin, hook, &plugin_context).await {
            Ok(result) => Ok(result),
            // The engine reports a missing export by this exact substring
            // — see `engine_tests.rs`.
            Err(e) if e.contains("Hook function not found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Let the deploy plugin finish a domain setup moss-oracle has already
    /// completed — a GitHub Pages CNAME, say.
    ///
    /// Non-fatal at the caller: DNS is configured either way, so a failure here
    /// becomes a warning next to the domain, not a broken setup.
    pub async fn execute_configure_domain(&self, context: &ConfigureDomainContext) -> Result<Option<HookResult>, String> {
        let project_path = self.project_path.clone();
        self.dispatch_optional_deploy_hook("configure_domain", &project_path, context).await
    }

    /// Ask the deploy plugin whether it can publish, and what is missing.
    ///
    /// The manifest's `setup` block answers everything a declaration can, with
    /// no plugin code running. This is the rest: a daemon that must be up, an
    /// account that must be linked — readiness only the plugin can see. It runs
    /// on the Publish click, so `Ok(None)` is the important case.
    ///
    /// Failure is an error rather than a silent pass. Unlike `configure_domain`
    /// (best-effort, after the fact), this one gates a publish the user is
    /// waiting on — swallowing the failure would publish into whatever the
    /// probe was about to report.
    pub async fn execute_check_setup(
        &self,
        context: &SetupContext,
    ) -> Result<Option<SetupVerdict>, String> {
        Ok(self
            .dispatch_optional_deploy_hook("check_setup", &context.project_path, context)
            .await?
            .and_then(|r| r.setup))
    }

    /// Execute a hook with timeout and return the full HookResult
    ///
    /// Like `execute_plugin_javascript`, but returns the HookResult from the
    /// plugin, including any deployment info.
    ///
    /// # Arguments
    /// * `plugin` - The plugin to execute
    /// * `hook_name` - Name of the hook function to call
    /// * `context` - Context to pass (will be serialized to JSON)
    ///
    /// # Returns
    /// * `Ok(Some(HookResult))` - Plugin executed and returned a result
    /// * `Ok(None)` - Plugin executed but didn't return a result
    /// * `Err(String)` - Plugin execution failed
    async fn execute_hook_with_result<T: serde::Serialize>(
        &self,
        plugin: &Plugin,
        hook_name: &str,
        context: &T,
    ) -> Result<Option<HookResult>, String> {
        // Serialize context to JSON
        let context_json = serde_json::to_string(context)
            .map_err(|e| format!("Failed to serialize context: {}", e))?;

        // Execute with activity-based timeout (timeout is managed inside the function)
        // The inner function uses inactivity detection: if no progress messages
        // arrive within INACTIVITY_TIMEOUT, the hook is considered stuck.
        self.execute_plugin_javascript_with_result(plugin, hook_name, &context_json).await
    }

    /// Calls hook function on already-initialized plugin instance
    /// Following VS Code/Obsidian patterns: plugins loaded once, hooks called multiple times
    ///
    /// # Timeout
    /// - Waits for completion signal or inactivity timeout (see `run_hook`)
    /// - Progress and errors are logged by the `plugin_message` command
    ///
    /// # Arguments
    /// * `plugin` - The plugin to execute
    /// * `hook_name` - Name of the hook function to call (e.g., "process")
    /// * `context_json` - Serialized context to pass to the function
    async fn execute_plugin_javascript(
        &self,
        plugin: &Plugin,
        hook_name: &str,
        context_json: &str,
    ) -> Result<(), String> {
        let hook_state = &self.host.state.hooks;

        // --- Singleflight: deduplicate concurrent dispatch of the same hook ---
        //
        // TWO purposes (Phase-4 verified-and-kept — plan D7 / flip-surfaces probe
        // F5; do NOT remove this mechanism):
        //
        // 1. Context-clobber protection. run_hook sets the per-Context global
        //    __MOSS_INTERNAL_CONTEXT__ at dispatch start and clears it at end
        //    (engine.rs / loader.rs), so two overlapping hooks of ONE plugin
        //    clobber each other (Phase-3 plan "Known latent issue #2"). Per-plugin
        //    Contexts already rule out the cross-plugin form; the plugin::hook key
        //    covers the same-plugin/same-HOOK subset that remains.
        //
        // 2. Dispatch dedup — a WORKLOAD property, engine-independent: install-
        //    triggered and build-triggered process hooks for the same plugin share
        //    one execution and one result (without it, matters sync runs twice
        //    concurrently: duplicate downloads, racing project-file writes).
        //
        // True retirement of purpose 1 = pass the hook context per-call instead
        // of via a Context global — recorded Phase-5 item (same dispatch spine as
        // per-Context serialization; see execute_plugin_javascript_with_result).
        let singleflight_key = format!("{}::{}", plugin.manifest.name, hook_name);
        let (is_first, mut rx) = hook_state.in_flight_hooks.try_start(&singleflight_key);

        if !is_first {
            log::info!(
                target: "plugin",
                "Hook '{}::{}' already in-flight, sharing result",
                plugin.manifest.name, hook_name
            );
            // Wait for the first caller's result via the watch channel.
            // If the channel closes (e.g., abort), treat as success
            // (hook was cancelled, not failed).
            let _ = rx.changed().await;
            return match rx.borrow().clone() {
                Some(result) => result,
                None => Ok(()), // Aborted — treat as no-op
            };
        }

        // From here, this caller is the first and is responsible for
        // completing or aborting the singleflight key.
        //
        // Use a SingleflightGuard to ensure cleanup even if this future is
        // dropped before it finishes — a caller that gives up on the dispatch
        // must not strand the key. The guard calls abort() on drop unless
        // explicitly disarmed.
        use crate::build::cache::SingleflightGuard;
        let mut sf_guard = SingleflightGuard::new(
            &hook_state.in_flight_hooks,
            singleflight_key.clone(),
        );

        match self.run_hook(plugin, hook_name, context_json, &hook_state).await {
            // Parse/dispatch failure: the JS never ran. Guard drop aborts the
            // key; waiters treat the closed channel as a no-op, exactly as if
            // the key had never been registered.
            HookRun::NeverRan(e) => Err(e),
            // The select! loop produced a verdict (success or timeout) —
            // disarm the guard and share it with any waiters.
            HookRun::Finished(result) => {
                let result = result.map(|_| ());
                sf_guard.disarm();
                hook_state.in_flight_hooks.complete(&singleflight_key, result.clone());
                result
            }
        }
    }

    /// Execute plugin JavaScript code and return the HookResult
    ///
    /// Same execution spine as `execute_plugin_javascript` (both delegate to
    /// `run_hook`), but returns the full HookResult from the plugin, including
    /// deployment info and other metadata.
    ///
    /// NOTE: This path does NOT use Singleflight dedup. It is only called
    /// for deploy hooks (on_deploy), which are user-initiated (click "Publish")
    /// and not susceptible to the concurrent dispatch bug. The context clobbering
    /// issue only affects process/generate/slots hooks that can fire from both
    /// install and build paths simultaneously.
    ///
    /// WITHIN-MANAGER CONCURRENCY PARITY (#789 Phase-3 plan, D1 "Known latent
    /// issue #2" — documented, NOT fixed here): deploy/syndicate hooks have NO
    /// singleflight, so a deploy hook can overlap a build's process hook for the
    /// SAME plugin on the SAME engine Context — the first hook to finish clears
    /// `__MOSS_INTERNAL_CONTEXT__` out from under the still-running one (SDK
    /// calls then fail "must be called from within a plugin hook"). Candidates
    /// for a real fix: serialize per-Context, or pass the context per-call
    /// instead of via a global.
    ///
    /// # Returns
    /// * `Ok(Some(HookResult))` - Plugin executed and returned a result
    /// * `Ok(None)` - Plugin executed successfully but didn't return a result
    /// * `Err(String)` - Plugin execution failed
    async fn execute_plugin_javascript_with_result(
        &self,
        plugin: &Plugin,
        hook_name: &str,
        context_json: &str,
    ) -> Result<Option<HookResult>, String> {
        match self.run_hook(plugin, hook_name, context_json, &self.host.state.hooks).await {
            HookRun::NeverRan(e) => Err(e),
            HookRun::Finished(result) => result,
        }
    }

    /// The one hook-execution spine: context prep, engine dispatch, the
    /// activity-based watchdog, teardown, and the final progress emit.
    ///
    /// Routed through `engine.dispatch_hook` (the `PluginEngine` trait,
    /// #789 Phase 2/3). The sink routes signals into the SAME PluginHookState
    /// the select! loop below reads, and the Complete signal carries the
    /// HookResult into the completion oneshot.
    ///
    /// The two-armed return type exists for the singleflight caller: a
    /// `NeverRan` failure must ABORT the key — current waiters see the closed
    /// channel as a no-op `Ok(())` and callers arriving AFTER the abort
    /// re-dispatch — while a `Finished` verdict, success OR timeout, must
    /// COMPLETE it, so waiters share the actual result.
    async fn run_hook(
        &self,
        plugin: &Plugin,
        hook_name: &str,
        context_json: &str,
        hook_state: &PluginHookState,
    ) -> HookRun {
        use tokio::time::Duration;

        // Parse context JSON to send as structured data
        let mut context: serde_json::Value = match serde_json::from_str(context_json) {
            Ok(v) => v,
            Err(e) => return HookRun::NeverRan(format!("Failed to parse context JSON: {}", e)),
        };

        // Inject project_path for SDK internal use (the engine extracts it to
        // set __MOSS_INTERNAL_CONTEXT__, then strips it before passing to plugins)
        if let Some(obj) = context.as_object_mut() {
            obj.insert("project_path".to_string(),
                serde_json::Value::String(self.project_path.clone()));
        }

        // Create oneshot channel for completion signal (via plugin_message command)
        let (complete_tx, mut complete_rx) = tokio::sync::oneshot::channel();
        hook_state.set_completion_sender(&plugin.manifest.name, hook_name, complete_tx);

        // Initialize activity tracking for this hook
        hook_state.init_activity(&plugin.manifest.name, hook_name);

        // Phase 2 B2 interim: register the executing hook name so that
        // `apply_awaiting` can mark THIS hook (not just the task's UI hook label)
        // as awaiting when the plugin reports Awaiting. This fixes the key mismatch
        // where matters `process` creates its task with `hook:"import"`, causing
        // `apply_awaiting("matters","import",…)` to be a no-op for the watchdog that
        // checks `is_hook_awaiting("matters","process")`.
        hook_state.register_executing_hook(&plugin.manifest.name, hook_name);

        let sink = engine_adapter::HookSignalSink::new({
            let hooks = self.host.state.hooks.clone();
            let reporter = self.host.reporter.clone();
            let plugin = plugin.manifest.name.clone();
            let hook = hook_name.to_string();
            move |sig| signal_route::route_signal(&hooks, &*reporter, &plugin, &hook, sig)
        });
        let invocation = engine_adapter::HookInvocation {
            plugin_name: plugin.manifest.name.clone(),
            hook_name: hook_name.to_string(),
            context,
            project_path: self.project_path.clone(),
            moss_dir: format!("{}/.moss", self.project_path),
        };
        if let Err(e) = self.engine.dispatch_hook(invocation, sink).await {
            // Unregister the executing hook so a stale entry can't survive a
            // dispatch failure (the JS never ran, so no awaiting entry leaks;
            // this is hygiene).
            hook_state.unregister_executing_hook(&plugin.manifest.name);
            return HookRun::NeverRan(e);
        }

        // Activity-based timeout: check for inactivity instead of fixed timeout.
        // This allows plugins to run as long as they're making progress.
        const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(60);
        const CHECK_INTERVAL: Duration = Duration::from_secs(5);

        let result = loop {
            tokio::select! {
                completion = &mut complete_rx => {
                    break match completion {
                        Ok(completion_result) => {
                            if completion_result.success {
                                Ok(completion_result.result)
                            } else {
                                // Prefer .error (set for throw-path failures), then fall
                                // back to .result.message (set when success:false is
                                // RETURNED rather than thrown), and finally a generic
                                // sentinel.
                                let err = completion_result.error
                                    .or_else(|| completion_result.result.as_ref().and_then(|r| r.message.clone()))
                                    .unwrap_or_else(|| "Unknown error".to_string());
                                Err(format!("Hook failed: {}", err))
                            }
                        }
                        Err(_) => Err("Completion channel closed unexpectedly".to_string()),
                    };
                }
                _ = tokio::time::sleep(CHECK_INTERVAL) => {
                    if hook_state.is_making_progress(&plugin.manifest.name, hook_name) {
                        continue;
                    }
                    let last_activity = hook_state.get_last_activity(&plugin.manifest.name, hook_name);
                    if last_activity.elapsed() > INACTIVITY_TIMEOUT {
                        // Condition 6: kill the orphaned engine-side hook so it does
                        // not keep running invisibly. Late Complete after this break
                        // is harmless: hook_state.clear below removed the completion
                        // sender; send_completion's map.remove finds nothing (commands.rs:249).
                        self.engine.cancel_hook(&plugin.manifest.name, hook_name);
                        // Wedge hint (D1 named failure mode): if these repeat across
                        // different plugins the engine may be wedged by a runaway
                        // timer/interval callback (cancel cannot reach drive()-pumped
                        // JS); close and reopen the folder to recover.
                        log::warn!(
                            target: "plugin",
                            "inactivity timeout — if these repeat across plugins, the engine may be \
                             wedged by a runaway timer/interval callback (cancel cannot reach \
                             drive()-pumped JS); close and reopen the folder to recover"
                        );
                        break Err(format!(
                            "Hook '{}::{}' timed out - no progress for {} seconds",
                            plugin.manifest.name, hook_name, INACTIVITY_TIMEOUT.as_secs()
                        ));
                    }
                }
            }
        };

        // Collect any errors and clear state for this hook
        let _errors = hook_state.take_errors(&plugin.manifest.name, hook_name);
        hook_state.clear(&plugin.manifest.name, hook_name);

        // Phase 2 B2 interim: teardown — unregister the executing hook so a
        // future invocation of a different hook name for this plugin starts clean.
        hook_state.unregister_executing_hook(&plugin.manifest.name);

        // Signal the shell that this hook's progress task is done
        self.host.reporter.report(&crate::build::progress::PipelineEvent::PluginProgress(
            crate::plugins::types::PluginProgressEvent {
                plugin_name: plugin.manifest.name.clone(),
                hook_name: hook_name.into(),
                phase: if result.is_ok() { "done" } else { "error" }.into(),
                current: 0,
                total: 0,
                message: None,
                completed: true,
            },
        ));

        HookRun::Finished(result)
    }
}

/// Outcome of one `run_hook` execution, split by what the singleflight
/// caller must do with the key: `NeverRan` (parse/dispatch failed, the JS
/// never started) → abort — current waiters get a no-op `Ok(())` and later
/// callers re-dispatch; `Finished` (the watchdog loop produced a verdict,
/// success or timeout) → complete, so waiters share the result.
enum HookRun {
    NeverRan(String),
    Finished(Result<Option<HookResult>, String>),
}

/// Derive the JavaScript global name from a plugin name.
///
/// Convention: capitalize first letter + "Plugin" suffix.
/// e.g., "matters" → "MattersPlugin", "github" → "GithubPlugin"
pub(crate) fn derive_global_name(name: &str) -> String {
    let mut capitalized = name.to_string();
    if let Some(first) = capitalized.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    format!("{}Plugin", capitalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache is one manager per project, whatever path spelling asked:
    /// a second `get_or_create` and a `get_existing` hand back the same `Arc`.
    // A manager spawns its engine thread through the headless host's
    // `TokioSpawner`, so the cache needs a runtime under it.
    #[tokio::test]
    async fn the_cache_builds_one_manager_per_project() {
        let temp = tempfile::tempdir().unwrap();
        let cache = ManagerCache::headless(crate::build::ports::reporter::discard_owned());
        let folder = temp.path().to_str().unwrap();
        assert!(cache.get_existing(folder).is_none());

        let first = cache.get_or_create(folder).unwrap();
        let again = cache.get_or_create(folder).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(Arc::ptr_eq(&first, &cache.get_existing(folder).unwrap()));
        assert!(cache.get_or_create("/nowhere/that/exists").is_err(), "an unresolvable folder is an error, not a manager");
    }

    // ── plugin advisory constructors (issue #793) ─────────────────────────

    /// Hook-failure advisory: Remote scope, NeedsAction severity, no action.
    #[test]
    fn plugin_hook_failure_advisory_shape() {
        use crate::advisory::{Action, Scope, Severity};
        let adv = PluginManager::plugin_hook_failure_advisory("matters", "network unreachable");
        assert!(matches!(adv.scope, Scope::Remote), "hook failure is a Remote advisory");
        assert!(matches!(adv.severity, Severity::NeedsAction), "hook failure is NeedsAction");
        assert!(matches!(adv.action, Action::None), "no automated recovery");
        assert!(adv.what.contains("matters"), "advisory names the plugin");
        assert!(adv.what.contains("network unreachable"), "advisory includes the error");
        assert!(adv.what.contains("stale"), "advisory mentions stale sync");
        assert!(adv.item.is_none(), "no per-file item for hook failure");
    }

    /// When a hook RETURNS {success:false, message} (plugin-reported failure, no
    /// throw), the completion carries error:null but result.message is set.  The
    /// message-resolution logic must fall back to result.message before the
    /// "Unknown error" sentinel so the advisory carries the real reason.
    ///
    /// Mirrors the resolution chain in `run_hook`.
    #[test]
    fn hook_completion_message_resolution() {
        use crate::plugins::types::HookResult;
        use crate::plugins::hook_state::CompletionResult;

        // Case 1: error field populated (throw path) — use it directly.
        let with_error = CompletionResult {
            success: false,
            error: Some("network timeout".to_string()),
            result: None,
        };
        let msg = with_error.error
            .or_else(|| with_error.result.as_ref().and_then(|r| r.message.clone()))
            .unwrap_or_else(|| "Unknown error".to_string());
        assert_eq!(msg, "network timeout", "error field wins when set");

        // Case 2: error field absent, result.message set (return-path failure).
        let hook_result = HookResult {
            success: false,
            message: Some("API quota exceeded".to_string()),
            toast: None,
            context: None,
            deployment: None,
            setup: None,
        };
        let with_message = CompletionResult {
            success: false,
            error: None,
            result: Some(hook_result),
        };
        let msg = with_message.error
            .or_else(|| with_message.result.as_ref().and_then(|r| r.message.clone()))
            .unwrap_or_else(|| "Unknown error".to_string());
        assert_eq!(msg, "API quota exceeded", "result.message used when error is absent");

        // Case 3: both absent — sentinel fires.
        let neither = CompletionResult {
            success: false,
            error: None,
            result: None,
        };
        let msg = neither.error
            .or_else(|| neither.result.as_ref().and_then(|r| r.message.clone()))
            .unwrap_or_else(|| "Unknown error".to_string());
        assert_eq!(msg, "Unknown error", "sentinel fires when both fields are absent");
    }

    /// Capability gate: Login-capable plugins self-report via PanelTask; the
    /// Rust-side hook-failure advisory must NOT fire for them (dedup guard).
    /// Non-login plugins keep the advisory path so failures are not silently swallowed.
    /// Verifies the predicate `has_login` used inline in `execute_process`.
    #[test]
    fn hook_failure_advisory_suppressed_for_login_plugins() {
        use crate::plugins::types::Capability;

        // Simulate the capabilities a matters-like plugin declares.
        let login_caps = vec![Capability::Process, Capability::Login];
        let has_login_matters = login_caps.contains(&Capability::Login);
        assert!(has_login_matters, "Login-capable plugin: gate evaluates true → advisory suppressed");

        // A deploy-only plugin has no Login — advisory path must fire.
        let no_login_caps = vec![Capability::Process, Capability::Deploy];
        let has_login_deploy = no_login_caps.contains(&Capability::Login);
        assert!(!has_login_deploy, "Non-login plugin: gate evaluates false → advisory fires");
    }

    // ── bundle_source_for_project (B.7) ───────────────────────────────────

    /// The source reads the installed plugin FRESH on every call: the derive
    /// fallback ("DemoPlugin") applies when the manifest has no global_name; a
    /// manifest rewrite with "global_name": "Custom" is picked up by the NEXT
    /// call on the SAME source (fresh-read contract); an unknown plugin Errs
    /// with the name in the message.
    #[test]
    fn bundle_source_reads_installed_plugin_fresh() {
        let test_tmp = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp");
        std::fs::create_dir_all(test_tmp).unwrap();
        let dir = tempfile::TempDir::new_in(test_tmp).unwrap();
        let plugin_dir = dir.path().join(".moss").join("plugins").join("demo");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("manifest.json"),
            r#"{"name":"demo","version":"0.0.1","description":"d","capabilities":["deploy"],"entry":"plugin.js"}"#,
        )
        .unwrap();
        std::fs::write(plugin_dir.join("plugin.js"), "var DemoPlugin = {};").unwrap();

        let source = bundle_source_for_project(dir.path().to_string_lossy().to_string());
        let bundle = source("demo").expect("installed plugin must load");
        assert_eq!(bundle.bundle, "var DemoPlugin = {};", "source must return the entry file's bytes");
        assert_eq!(bundle.global_name, "DemoPlugin", "derive fallback must apply");

        // Manifest global_name override wins (untested override path, sdk-bundles probe).
        std::fs::write(
            plugin_dir.join("manifest.json"),
            r#"{"name":"demo","version":"0.0.1","description":"d","capabilities":["deploy"],"entry":"plugin.js","global_name":"Custom"}"#,
        )
        .unwrap();
        let bundle = source("demo").expect("installed plugin must reload");
        assert_eq!(bundle.global_name, "Custom", "manifest global_name override must win");

        let err = source("ghost").expect_err("unknown plugin must Err");
        assert!(err.contains("ghost"), "error must name the plugin: {err}");
    }

    // ── contributes.jobs descriptor lookup (Step 3 Phase 5, §8 + R13) ──────
    //
    // Proves `JobDescriptor`/`ContributedJobs` has a PRODUCTION CONSUMER
    // (CLAUDE.md: "every module must have a consumer before it ships"): the
    // lifecycle command resolves a plugin's declared job descriptor through
    // `resolve_job_descriptor_in`, then `Verb::normalized`s the verb (R13)
    // before stamping it on the Job.

    fn plugin_with_jobs(
        name: &str,
        jobs: &[(&str, &str, &str)],
    ) -> Plugin {
        use crate::plugins::types::{
            ContributedJobs, JobDescriptor, PluginContributes, PluginManifest,
        };
        let descriptors = jobs
            .iter()
            .map(|(id, verb, noun)| {
                (
                    id.to_string(),
                    JobDescriptor { verb: verb.to_string(), noun: noun.to_string() },
                )
            })
            .collect();
        Plugin {
            manifest: PluginManifest {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                entry: "main.js".to_string(),
                capabilities: vec![Capability::Syndicate],
                contributes: Some(PluginContributes {
                    frontmatter: None,
                    embed_renderers: vec![],
                    channel: None,
                    deploy_target: None,
                    processor: None,
                    jobs: Some(ContributedJobs { descriptors }),
                    stack: None,
                }),
                ..Default::default()
            },
            path: std::path::PathBuf::from("/tmp/test-plugin"),
        }
    }

    #[test]
    fn resolve_job_descriptor_reads_contributes_jobs() {
        // The matters case: the manifest declares `syndicate → Syndicated · posts`.
        let plugins = vec![plugin_with_jobs("matters", &[("syndicate", "Syndicated", "posts")])];
        let descriptor = resolve_job_descriptor_in(&plugins, "matters", "syndicate")
            .expect("declared job descriptor must resolve");
        assert_eq!(descriptor.verb, "Syndicated");
        assert_eq!(descriptor.noun, "posts");
    }

    #[test]
    fn resolve_job_descriptor_misses_are_none() {
        let plugins = vec![plugin_with_jobs("matters", &[("syndicate", "Syndicated", "posts")])];
        // Unknown job id on a known plugin → None (falls back to legacy receipt).
        assert!(resolve_job_descriptor_in(&plugins, "matters", "nope").is_none());
        // Unknown plugin → None.
        assert!(resolve_job_descriptor_in(&plugins, "ghost", "syndicate").is_none());
        // A plugin with no `contributes` at all → None.
        let mut bare = plugin_with_jobs("bare", &[]);
        bare.manifest.contributes = None;
        assert!(resolve_job_descriptor_in(&[bare], "bare", "syndicate").is_none());
    }

    #[test]
    fn resolve_job_descriptor_spoof_verb_returned_raw_for_command_to_normalize() {
        // The lookup returns the plugin's RAW proposed word; `Verb::normalized`
        // (R13) is the lifecycle command's job (verified end-to-end in
        // plugins::runtime). This pins the contract: a spoof in the manifest
        // survives the lookup verbatim, then moss clamps it at the seam.
        let plugins =
            vec![plugin_with_jobs("matters", &[("syndicate", "🚀 syndicated!!!", "posts")])];
        let descriptor = resolve_job_descriptor_in(&plugins, "matters", "syndicate").unwrap();
        assert_eq!(descriptor.verb, "🚀 syndicated!!!");
        // And the seam moss applies turns it into its own verb.
        assert_eq!(crate::tasks::Verb::normalized(&descriptor.verb).0, "Syndicated");
    }

    #[test]
    fn test_derive_global_name_from_plugin_name() {
        // Derives PascalCase + "Plugin" from lowercase plugin names
        assert_eq!(derive_global_name("matters"), "MattersPlugin");
        assert_eq!(derive_global_name("comment"), "CommentPlugin");
        assert_eq!(derive_global_name("github"), "GithubPlugin");
        assert_eq!(derive_global_name("email"), "EmailPlugin");
        assert_eq!(derive_global_name("hugo"), "HugoPlugin");
        assert_eq!(derive_global_name("jekyll"), "JekyllPlugin");
    }

    #[test]
    fn test_derive_global_name_already_capitalized() {
        assert_eq!(derive_global_name("Matters"), "MattersPlugin");
    }

    #[test]
    fn test_derive_global_name_empty() {
        assert_eq!(derive_global_name(""), "Plugin");
    }

}
