//! Hook-context config: the one place a plugin's settings are resolved.
//!
//! A hook context reaches a plugin with a `config` map that no caller can fill
//! in: `build_deploy_context` and the syndicate builder do not know which
//! plugin will run until a dispatcher picks one, and the process dispatcher
//! fans a single context out to many plugins. So every caller hands
//! over an empty map and the dispatchers in `manager.rs` write it through
//! [`context_for_plugin`] here.

use crate::plugins::setup::SetupContext;
use crate::plugins::types::{
    ConfigureDomainContext, DeployContext, Plugin, ProcessContext, SyndicateContext,
};
use std::collections::HashMap;

/// A hook context whose settings slot the dispatcher fills in.
///
/// The hook contexts each carry a `config` map that only the dispatcher
/// can populate, so this is the seam `PluginManager::context_for_plugin` writes
/// through — one owner instead of the same three lines per hook.
pub(super) trait HookContext: Clone {
    fn config_slot(&mut self) -> &mut HashMap<String, serde_json::Value>;

    /// Runs after the dispatcher fills `config`. The default is nothing; a
    /// context that carries the same map under a second, contract-named key
    /// mirrors it here.
    fn on_config_filled(&mut self) {}
}

macro_rules! impl_hook_context {
    ($($ty:ty),+ $(,)?) => {$(
        impl HookContext for $ty {
            fn config_slot(&mut self) -> &mut HashMap<String, serde_json::Value> {
                &mut self.config
            }
        }
    )+};
}

impl_hook_context!(
    ProcessContext,
    DeployContext,
    SyndicateContext,
    ConfigureDomainContext,
);

impl HookContext for SetupContext {
    fn config_slot(&mut self) -> &mut HashMap<String, serde_json::Value> {
        &mut self.config
    }

    /// The setup contract's name for the resolved plain fields is `settings`;
    /// `config` stays as the deprecated alias pre-contract hooks read. One
    /// resolution, two keys on the wire.
    fn on_config_filled(&mut self) {
        self.settings = self.config.clone();
    }
}

// Every hook-dispatching method on the manager runs a real engine, so none of
// them is driven from a unit test. Resolving each hook's config inline
// therefore put the resolution itself out of the suite's reach: a call site
// that quietly dropped the manifest defaults stayed green. Everything here is
// a plain function over a `Plugin`, so a test drives the real production code
// with a real discovered plugin. Keep the call sites one-liners — do not read
// plugin config inline again.

/// Resolve the config a hook context carries for `plugin`: persisted values
/// (`.moss/plugins/<name>/config.{json,toml}`) overlaid on the manifest's
/// declared defaults, so a key the user never touched still reaches the hook
/// with the plugin author's default instead of missing.
pub(super) fn hook_config_for_plugin(
    project_path: &str,
    plugin: &Plugin,
) -> HashMap<String, serde_json::Value> {
    crate::plugins::discovery::get_plugin_config_with_defaults(
        project_path,
        &plugin.manifest.name,
        &plugin.manifest.config_defaults(),
    )
}

/// The hook context `plugin` receives: the caller's context with `config`
/// replaced by this plugin's resolved config.
///
/// Every hook routes through here, because no caller can resolve its own
/// config: `build_deploy_context` and the syndicate builder do not know
/// which plugin will run until a dispatcher below picks one, and the
/// process dispatcher fans one context out to many plugins. So the
/// callers hand over an empty map and this is the single writer.
pub(super) fn context_for_plugin<C: HookContext>(project_path: &str, context: &C, plugin: &Plugin) -> C {
    let mut plugin_context = context.clone();
    *plugin_context.config_slot() = hook_config_for_plugin(project_path, plugin);
    plugin_context.on_config_filled();
    plugin_context
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::types::{ProjectInfo, TriggerContext};


    /// Install a real plugin into `<project>/.moss/plugins/<name>/` with the
    /// given manifest-declared `config` defaults, then return the plugin set the
    /// way production gets it — parsed off disk by `discover_plugins`, not
    /// hand-built. Hook-context tests need the real thing: a hand-built `Plugin`
    /// would prove the merge helper works while leaving the question of whether
    /// production hands it the manifest defaults unanswered.
    fn discover_with_installed_plugin(
        project: &std::path::Path,
        name: &str,
        capability: &str,
        manifest_config: serde_json::Value,
    ) -> Vec<Plugin> {
        let dir = project.join(".moss").join("plugins").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "entry": "main.js",
                "capabilities": [capability],
                "config": manifest_config,
            })
            .to_string(),
        )
        .unwrap();
        // load_plugin validates that the declared entry file exists.
        std::fs::write(dir.join("main.js"), "").unwrap();

        crate::plugins::bundled::discover_plugins(project.to_str().unwrap())
            .expect("discovery must succeed")
    }

    fn find_plugin<'a>(plugins: &'a [Plugin], name: &str) -> &'a Plugin {
        plugins
            .iter()
            .find(|p| p.manifest.name == name)
            .unwrap_or_else(|| panic!("plugin '{name}' must be discovered"))
    }

    // These drive the production function the hook dispatchers call
    // (`context_for_plugin`) with a real plugin discovered off disk. Dropping
    // the manifest defaults anywhere in that chain turns them red — which a
    // test that only exercised the `discovery` merge helper could not do.

    /// The per-plugin `ProcessContext` carries persisted values from
    /// `.moss/plugins/<name>/config.toml` AND manifest-declared defaults for
    /// keys the user never persisted, without mutating the shared context.
    #[test]
    fn process_context_carries_persisted_config_over_manifest_defaults() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();

        let plugins = discover_with_installed_plugin(
            temp_dir.path(),
            "comments",
            "process",
            serde_json::json!({
                "moderation": "queue",
                "server_url": "https://declared.example.com",
            }),
        );
        let plugin_config_dir = temp_dir.path().join(".moss").join("plugins").join("comments");
        std::fs::write(
            plugin_config_dir.join("config.toml"),
            r#"
    server_url = "https://comments.example.com"
    site_name = "my-blog"
    "#,
        )
        .unwrap();

        // The shared context spawn_process_hooks builds: empty config.
        let context = ProcessContext {
            project_path: project_path.to_string(),
            moss_dir: format!("{}/.moss", project_path),
            project_info: ProjectInfo {
                total_files: 1,
                homepage_file: None,
                folder_name: None,
                site_name: None,
                lang: "en".into(),
            },
            config: HashMap::new(),
            trigger: TriggerContext::Background,
        };

        let plugin_context =
            context_for_plugin(project_path, &context, find_plugin(&plugins, "comments"));

        assert_eq!(
            plugin_context.config.get("server_url").and_then(|v| v.as_str()),
            Some("https://comments.example.com"),
            "persisted value must win over the declared default"
        );
        assert_eq!(
            plugin_context.config.get("site_name").and_then(|v| v.as_str()),
            Some("my-blog"),
            "persisted-only key must survive the merge"
        );
        assert_eq!(
            plugin_context.config.get("moderation"),
            Some(&serde_json::json!("queue")),
            "manifest default must reach the process context for an unset key"
        );
        assert_eq!(plugin_context.trigger, TriggerContext::Background);
        assert!(
            context.config.is_empty(),
            "the shared context must stay untouched"
        );
    }

    /// Nothing persisted and nothing declared ⇒ empty config, not an error.
    #[test]
    fn process_context_empty_config_when_nothing_persisted_or_declared() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let plugins = discover_with_installed_plugin(
            temp_dir.path(),
            "comments",
            "process",
            serde_json::json!({}),
        );

        let context = ProcessContext {
            project_path: project_path.to_string(),
            moss_dir: format!("{}/.moss", project_path),
            project_info: ProjectInfo {
                total_files: 1,
                homepage_file: None,
                folder_name: None,
                site_name: None,
                lang: "en".into(),
            },
            config: HashMap::new(),
            trigger: TriggerContext::Background,
        };

        let plugin_context =
            context_for_plugin(project_path, &context, find_plugin(&plugins, "comments"));

        assert!(
            plugin_context.config.is_empty(),
            "process context should have empty config when nothing is persisted or declared"
        );
    }

    /// The deploy hook's config. This is the regression that mattered in the
    /// field: `build_deploy_context` hands the manager an empty map, so before
    /// the manager filled it every deploy plugin ran with no settings at all —
    /// the github plugin never saw its own `video_max_size_mb` default, and a
    /// user's persisted value was silently discarded.
    #[test]
    fn deploy_context_carries_persisted_config_over_manifest_defaults() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let plugins = discover_with_installed_plugin(
            temp_dir.path(),
            "github",
            "deploy",
            serde_json::json!({ "auto_commit": true, "video_max_size_mb": 75 }),
        );
        std::fs::write(
            temp_dir
                .path()
                .join(".moss")
                .join("plugins")
                .join("github")
                .join("config.toml"),
            "auto_commit = false\n",
        )
        .unwrap();

        // The context `build_deploy_context` builds: empty config.
        let context = DeployContext {
            project_info: ProjectInfo {
                total_files: 1,
                homepage_file: None,
                folder_name: None,
                site_name: None,
                lang: "en".into(),
            },
            site_files: vec!["index.html".to_string()],
            config: HashMap::new(),
            domain: None,
        };

        let plugin_context =
            context_for_plugin(project_path, &context, find_plugin(&plugins, "github"));

        assert_eq!(
            plugin_context.config.get("auto_commit"),
            Some(&serde_json::json!(false)),
            "persisted value must win over the declared default"
        );
        assert_eq!(
            plugin_context.config.get("video_max_size_mb"),
            Some(&serde_json::json!(75)),
            "manifest default must reach the deploy hook for an unset key"
        );
        assert_eq!(plugin_context.site_files, context.site_files);
        assert!(
            context.config.is_empty(),
            "the caller's context must stay untouched"
        );
    }

    /// The setup context carries its one resolved map under BOTH names: the
    /// contract's `settings` and the pre-contract `config` a shipped hook
    /// still reads. A fill that reached only one would hand a migrated plugin
    /// an empty settings map.
    #[test]
    fn setup_context_mirrors_config_into_settings() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let plugins = discover_with_installed_plugin(
            temp_dir.path(),
            "ipfs",
            "deploy",
            serde_json::json!({ "provider": "local" }),
        );

        let context = crate::plugins::setup::SetupContext {
            project_path: project_path.to_string(),
            settings: HashMap::new(),
            config: HashMap::new(),
            action: None,
            values: HashMap::new(),
        };
        let plugin_context =
            context_for_plugin(project_path, &context, find_plugin(&plugins, "ipfs"));

        assert_eq!(plugin_context.settings.get("provider"), Some(&serde_json::json!("local")));
        assert_eq!(plugin_context.settings, plugin_context.config, "one map, two names");
    }

    /// Syndication dispatches to several channels, so each one gets its own
    /// config rather than a single shared serialization.
    #[test]
    fn syndicate_context_carries_the_plugins_own_config() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_str().unwrap();
        let plugins = discover_with_installed_plugin(
            temp_dir.path(),
            "matters",
            "syndicate",
            serde_json::json!({ "add_canonical_link": true }),
        );
        std::fs::write(
            temp_dir
                .path()
                .join(".moss")
                .join("plugins")
                .join("matters")
                .join("config.toml"),
            "add_canonical_link = false\n",
        )
        .unwrap();

        let context = SyndicateContext {
            project_path: project_path.to_string(),
            moss_dir: format!("{}/.moss", project_path),
            output_dir: format!("{}/.moss/site", project_path),
            project_info: ProjectInfo {
                total_files: 1,
                homepage_file: None,
                folder_name: None,
                site_name: None,
                lang: "en".into(),
            },
            site_files: Vec::new(),
            articles: Vec::new(),
            deployment: None,
            config: HashMap::new(),
            trigger: TriggerContext::Background,
        };

        let plugin_context =
            context_for_plugin(project_path, &context, find_plugin(&plugins, "matters"));

        assert_eq!(
            plugin_context.config.get("add_canonical_link"),
            Some(&serde_json::json!(false)),
            "a persisted opt-out must reach the syndicate hook"
        );
        assert!(context.config.is_empty(), "the shared context must stay untouched");
    }
}
