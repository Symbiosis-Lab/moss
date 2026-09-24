//! Core types for the plugin system

pub use super::contributions::{
    ChannelContribution, ContributedFrontmatter, ContributedJobs, DeployTargetContribution,
    EmbedRendererContribution, JobDescriptor, PluginContributes,
};
use super::setup::SetupVerdict;
use crate::build::scan::article_map::ArticleInfo;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::path::PathBuf;

/// The one host capability `requires` can grant, and the prefix of its named
/// form: `execute_binary:<basename>` grants one executable, bare
/// `execute_binary` grants every one (deprecated, 2026-08-30). Written once so
/// the gate that enforces the grammar (`engine::host_fns::grants`) and the
/// settings page that reports it cannot drift apart.
pub const EXECUTE_BINARY_GRANT: &str = "execute_binary";

/// Plugin manifest - describes plugin metadata and capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name (e.g., "matters")
    pub name: String,

    /// Plugin version (semver)
    pub version: String,

    /// Human-readable description
    pub description: Option<String>,

    /// Plugin author
    pub author: Option<String>,

    /// Entry point file (relative to plugin directory)
    pub entry: String,

    /// Plugin capabilities - which hooks this plugin implements
    /// Each capability maps to a hook function with the same name
    #[serde(default)]
    pub capabilities: Vec<Capability>,

    /// Global name for JavaScript export
    /// This is the name of the global variable the plugin creates
    /// Default: uses plugin name converted to PascalCase + "Plugin"
    #[serde(default)]
    pub global_name: Option<String>,

    /// Plugin-specific configuration values (e.g., login_url, api_endpoint)
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,

    /// Plugin-specific configuration schema (optional, for validation)
    #[serde(default)]
    pub config_schema: Option<HashMap<String, String>>,

    /// Human-readable name for settings UI section title (e.g., "Comments")
    #[serde(default)]
    pub display_name: Option<String>,

    /// Field name to display label mapping for settings UI (e.g., {"enabled": "Enable Comments"})
    #[serde(default)]
    pub config_labels: Option<HashMap<String, String>>,

    /// Field name to help text mapping for settings UI (e.g., {"enabled": "Show comments on articles"})
    #[serde(default)]
    pub config_descriptions: Option<HashMap<String, String>>,

    /// Field name to its allowed values, for a `config_schema` field declared
    /// `"enum"` (e.g. {"provider": ["pinata", "web3storage"]}). A per-option
    /// display label is an entry in `config_labels` keyed `"<field>.<value>"`;
    /// without one the value is shown as written.
    #[serde(default)]
    pub config_options: Option<HashMap<String, Vec<String>>>,

    /// Field name to placeholder text mapping for settings UI (e.g., {"api_key": "Enter your API key"})
    #[serde(default)]
    pub config_placeholders: Option<HashMap<String, String>>,

    /// Optional icon path (relative to plugin directory)
    /// If not specified, falls back to convention-based paths: icon.svg, icon.png, logo.svg, logo.png
    #[serde(default)]
    pub icon: Option<String>,

    /// Domain for cookie access (e.g., "matters.town")
    /// This restricts the plugin to only access cookies from this domain
    /// Required for plugins that need cookie-based authentication
    #[serde(default)]
    pub domain: Option<String>,

    /// Full set of domains this plugin operates on (e.g.,
    /// `["matters.town", "matters.icu"]` for prod + staging Matters).
    /// Used by the scope-wide cookie clear (`resolve_plugin_domains`) so a
    /// force-fresh login wipes EVERY environment's cookies, not just the
    /// single active `domain`. Get/set cookie semantics keep using the single
    /// active `domain`. Absent for single-domain plugins (falls back to
    /// `[domain]`).
    #[serde(default)]
    pub domains: Option<Vec<String>>,

    /// Optional schema contributions (frontmatter fields, etc.)
    /// Follows the VS Code `contributes.configuration` pattern: plugins declare
    /// field definitions in their manifest; moss merges them into the active schema
    /// at runtime. See docs/reference/plugin-schema-contributions.md.
    #[serde(default)]
    pub contributes: Option<PluginContributes>,

    /// Minimum moss version this plugin supports (semver). The registry client
    /// checks it at install and load; a moss older than this field's
    /// introduction ignores it entirely, so load-time protection only exists on
    /// versions that ship the check. Absent = no lower bound.
    #[serde(default)]
    pub min_moss_version: Option<String>,

    /// Source repository URL. Display/provenance only.
    #[serde(default)]
    pub repository: Option<String>,

    /// Homepage URL. Display only.
    #[serde(default)]
    pub homepage: Option<String>,

    /// Legacy alias for `contributes.stack` — the plugin also declares its
    /// pin there now (S3); this stays as the pre-S3 need marker.
    #[serde(default)]
    pub requires_stack: bool,

    /// Not ready to be offered by default: the catalog omits this channel
    /// unless preview features are on. Travels to the registry index as the
    /// entry's `preview` key — `registry::channel_is_listed`, ADR-053.
    #[serde(default)]
    pub preview: bool,

    /// Host capabilities this plugin is granted; absent or unlisted = REFUSED.
    /// Named per binary since 2026-08-30: `"execute_binary:<basename>"` grants
    /// one executable; the bare `"execute_binary"` blanket is a deprecated
    /// alias granting everything, with a warning per run. Enforced fail-closed
    /// at the QuickJS seam (`require_binary_grant`, engine/host_fns/grants.rs);
    /// `moss-plugins`' `validate.yml` checks the same field. The keystore is
    /// NOT gated — a caller signs only with its own scoped key (ADR-032).
    #[serde(default)]
    pub requires: Option<Vec<String>>,
}

impl PluginManifest {
    /// Check if this plugin has a specific capability
    pub fn has_capability(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// The executables `requires` names, as basenames, in declaration order.
    ///
    /// Empty means this manifest names none — which is not the same as
    /// granting none, because the deprecated blanket token grants every binary
    /// while naming nothing. Ask [`Self::grants_any_binary`] to tell those
    /// apart. A `requires` entry that is neither the blanket token nor
    /// `execute_binary:<basename>` grants nothing today, so it names no binary
    /// here either: the list must not over-state what the gate will allow.
    pub fn granted_binaries(&self) -> Vec<String> {
        let prefix = format!("{EXECUTE_BINARY_GRANT}:");
        self.requires
            .iter()
            .flatten()
            .filter_map(|entry| entry.strip_prefix(&prefix))
            .filter(|basename| !basename.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Whether the manifest carries the blanket `execute_binary` token, which
    /// still grants every executable with a per-run deprecation warning.
    pub fn grants_any_binary(&self) -> bool {
        self.requires
            .iter()
            .flatten()
            .any(|entry| entry == EXECUTE_BINARY_GRANT)
    }
}

/// Plugin capabilities - what hooks a plugin implements
///
/// Each capability maps directly to a hook function that moss will call.
/// Plugins declare which capabilities they implement in their manifest.
///
/// `generate` and `enhance` were removed 2026-08-29 (ADR-055): both were
/// declared here for three months and implemented by nothing — no bundled,
/// WIP or archived plugin ever carried either. They remain designed, in the
/// docs the ADR cites; an implementer re-adds the variant with its first
/// real caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    /// Pre-process files before generation (multiple plugins allowed)
    /// Hook function: `process(ctx)`
    Process,

    /// Deploy to hosting platforms (single plugin only)
    /// Hook function: `deploy(ctx)`
    Deploy,

    /// POSSE distribution after deployment (multiple plugins allowed)
    /// Hook function: `syndicate(ctx)`
    Syndicate,

    /// Import content from an external source into the project folder
    /// (e.g., Matters profile, RSS feed, URL scrape). Multiple plugins allowed.
    /// Hook function: `import(ctx)`
    ///
    /// Added 2026-05-28 alongside `PluginHook::Import` to support the
    /// onboarding flow's "Plugin" card (per ADR-015 and the onboarding spec).
    Import,

    /// Plugin requires a user login to operate and exposes a standalone
    /// `login` export that moss invokes via `connect_account`.
    ///
    /// Declaring this capability causes moss to:
    ///   1. Show a connection-status row in the plugin's settings section.
    ///   2. Emit `MattersNeedsConnection` (or a generic equivalent) when the
    ///      folder is opened with no bound session, triggering auto-open-once.
    ///
    /// Added 2026-06-23 (Phase 4b) — currently used only by the Matters plugin.
    /// Does NOT map to a `hook_name()` (it's not a build hook).
    Login,
}

impl Capability {
    /// Get the hook function name for this capability.
    ///
    /// Total: every capability has a name. `Login` returns `"login"` even
    /// though it runs via `connect_account` out-of-band rather than as a
    /// build hook.
    pub fn hook_name(&self) -> &'static str {
        match self {
            Capability::Process => "process",
            Capability::Deploy => "deploy",
            Capability::Syndicate => "syndicate",
            Capability::Import => "import",
            // Login is dispatched out-of-band via connect_account; it has no
            // named build hook. Return the export name for informational use.
            Capability::Login => "login",
        }
    }
}

/// Plugin hook — the set of operations a plugin can perform.
///
/// Strict subset of `TaskKind` (introduced in T1 per ADR-015): exactly the
/// kinds plugins can produce. Internal-only kinds (Save, Validate, Lint,
/// Format, Resolve, Build, Rebuild, AssetTransform) live in `TaskKind` and
/// are NOT in `PluginHook` because plugins cannot perform those operations.
///
/// Each variant corresponds 1:1 with a `Capability` variant and a hook
/// function name. The router in `plugins::runtime` (T1) takes a
/// `(PluginHook, TriggerContext)` and emits a `(TaskScope, TaskKind,
/// TaskTone)` tuple, with `impl From<PluginHook> for TaskKind` providing
/// the kind mapping.
///
/// **Closed enum.** Adding a variant is a compile-time event that requires
/// updating every exhaustive match (notably the router). No wildcard arms.
///
/// See [ADR-015](../../../../../docs/decisions/ADR-015-panel-task-primitive.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PluginHook {
    /// Import content from an external source.
    Import,
    /// Publish content to the active distribution surface.
    Publish,
    /// Deploy the built site to a hosting platform.
    Deploy,
    /// POSSE distribution after deployment.
    Syndicate,
    /// Pre-process files before generation.
    Process,
}

impl PluginHook {
    /// Return the canonical hook name string used in PluginHookState keys and
    /// the watchdog activity map. Mirrors `Capability::hook_name()`.
    pub fn hook_name(&self) -> &'static str {
        match self {
            PluginHook::Import => "import",
            PluginHook::Publish => "publish",
            PluginHook::Deploy => "deploy",
            PluginHook::Syndicate => "syndicate",
            PluginHook::Process => "process",
        }
    }
}

/// Trigger context — *why* the plugin task was invoked.
///
/// Pairs with `PluginHook` as the two halves of `PluginTaskSignal` (T1).
/// Same (hook, trigger) cross product feeds the exhaustive router that
/// decides UI surface (`TaskScope`) and `TaskTone`.
///
/// **Closed enum.** Adding a variant is a compile-time event — every router
/// row must be updated explicitly. See [ADR-015 § Why the router is
/// exhaustive](../../../../../docs/decisions/ADR-015-panel-task-primitive.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TriggerContext {
    /// User is in the empty-folder onboarding cards.
    OnboardingFlow,
    /// User triggered the task manually from settings UI.
    SettingsManual,
    /// Scheduled / automatic / batched (no immediate user gesture).
    Background,
    /// One-off syndicate of a single article.
    ManualOne,
}

/// Plugin context - data passed between hooks during build
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PluginContext {
    /// Absolute path to the project folder being built
    pub project_path: String,

    /// Absolute path to .moss directory
    pub moss_dir: String,

    /// Absolute path to output directory (.moss/site)
    pub output_dir: String,

    /// Project structure information
    pub project_info: ProjectInfo,

    /// Articles being processed (for syndication)
    #[serde(default)]
    pub articles: Vec<ArticleInfo>,

    /// Deployment result (populated after deployment)
    pub deployment: Option<DeploymentInfo>,

    /// Plugin configuration from .moss/config.toml
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// Context for process capability (pre-processing before generation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessContext {
    /// Absolute path to the project folder being built
    pub project_path: String,

    /// Absolute path to .moss directory
    pub moss_dir: String,

    /// Project structure information
    pub project_info: ProjectInfo,

    /// Plugin configuration from .moss/config.toml
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,

    /// Why moss is invoking this hook. moss owns this context (ADR-015):
    /// the plugin reads it to declare task intent via `startTask`, it does
    /// NOT guess it. Onboarding card → `OnboardingFlow` (drives the ambient
    /// hairline); every build/preview rebuild → `Background` (quiet). Defaults
    /// to `Background` for any caller that doesn't set it explicitly.
    #[serde(default)]
    pub trigger: TriggerContext,
}

impl Default for TriggerContext {
    /// Background is the quietest surface (Workspace + Ambient); the safe
    /// fallback when no caller stamps an explicit trigger.
    fn default() -> Self {
        TriggerContext::Background
    }
}

impl ProcessContext {
    /// Construct a `ProcessContext`, deriving `moss_dir` from `project_path`.
    ///
    /// The `trigger` is the load-bearing input (ADR-015): it decides whether the
    /// plugin's tasks reach the ambient hairline (`OnboardingFlow`) or the quiet
    /// Workspace surface (`Background`). Prefer the named constructors
    /// [`ProcessContext::for_onboarding`] / [`ProcessContext::for_build`] at call
    /// sites so the trigger choice is explicit and regression-tested.
    pub fn new(project_path: String, project_info: ProjectInfo, trigger: TriggerContext) -> Self {
        let moss_dir = format!("{}/.moss", project_path);
        ProcessContext {
            project_path,
            moss_dir,
            project_info,
            config: HashMap::new(),
            trigger,
        }
    }

    /// Context for the post-install hook (user just connected a plugin via the
    /// onboarding card). Stamps `OnboardingFlow` so plugin import tasks route to
    /// `(ActionPanel, Ambient)` and animate the breadcrumb hairline.
    pub fn for_onboarding(project_path: String, project_info: ProjectInfo) -> Self {
        Self::new(project_path, project_info, TriggerContext::OnboardingFlow)
    }

    /// Context for the build / preview / rebuild path (no deliberate user
    /// gesture). Stamps `Background` so plugin tasks route to the quiet
    /// `(Workspace, Ambient)` surface — NOT the hairline.
    pub fn for_build(project_path: String, project_info: ProjectInfo) -> Self {
        Self::new(project_path, project_info, TriggerContext::Background)
    }
}

#[cfg(test)]
mod process_context_trigger_tests {
    use super::*;

    fn sample_info() -> ProjectInfo {
        ProjectInfo {
            total_files: 0,
            homepage_file: None,
            folder_name: None,
            site_name: None,
            lang: "en".into(),
        }
    }

    /// Regression guard for the headline onboarding fix: the onboarding
    /// constructor MUST stamp `OnboardingFlow` (the only trigger that routes
    /// plugin import tasks to the ActionPanel/Ambient breadcrumb hairline).
    /// If this flips to Background, the hairline goes dead in production —
    /// the exact bug this branch fixed.
    #[test]
    fn for_onboarding_stamps_onboarding_flow() {
        let ctx = ProcessContext::for_onboarding("/site".to_string(), sample_info());
        assert_eq!(ctx.trigger, TriggerContext::OnboardingFlow);
        assert_eq!(ctx.moss_dir, "/site/.moss");
    }

    /// The build/preview/rebuild path MUST stay `Background` (quiet Workspace
    /// surface) so routine rebuilds don't spuriously animate the hairline.
    #[test]
    fn for_build_stamps_background() {
        let ctx = ProcessContext::for_build("/site".to_string(), sample_info());
        assert_eq!(ctx.trigger, TriggerContext::Background);
    }

    /// An absent `trigger` (older moss serializing without the field) must
    /// deserialize to the quiet `Background` default, never to a loud surface.
    #[test]
    fn absent_trigger_deserializes_to_background() {
        let json = r#"{
            "project_path": "/site",
            "moss_dir": "/site/.moss",
            "project_info": { "total_files": 0, "homepage_file": null, "site_name": null, "lang": "en" }
        }"#;
        let ctx: ProcessContext = serde_json::from_str(json).expect("deserialize");
        assert_eq!(ctx.trigger, TriggerContext::Background);
    }

    /// The wire value the plugin/router sees must be snake_case (matches the
    /// TS `TriggerContext` union and `route_plugin_task`'s inputs).
    #[test]
    fn trigger_serializes_to_snake_case() {
        let ctx = ProcessContext::for_onboarding("/site".to_string(), sample_info());
        let json = serde_json::to_string(&ctx).expect("serialize");
        assert!(json.contains(r#""trigger":"onboarding_flow""#), "got: {json}");
    }
}


/// Context for deploy capability (publishing to hosting platforms)
///
/// Note: Path fields (project_path, moss_dir, output_dir) are intentionally omitted.
/// Plugins access files through SDK functions (readSiteFile, readProjectFile, etc.)
/// which resolve paths via the runtime's internal context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployContext {
    /// Project structure information
    pub project_info: ProjectInfo,

    /// List of files to deploy (relative to output_dir)
    pub site_files: Vec<String>,

    /// Plugin configuration from .moss/config.toml
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,

    /// Custom domain from .moss/state.toml [deployment] section (if configured)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

/// Context for configure_domain hook (custom domain setup on deploy platform)
///
/// Called after DNS records are configured via moss-seta. Allows deploy plugins
/// to perform platform-specific domain setup (e.g., GitHub Pages CNAME configuration).
///
/// This is NOT a separate capability - it's an optional hook on Deploy-capable plugins.
///
/// Note: Path fields (project_path, moss_dir) are intentionally omitted.
/// Plugins access files through SDK functions which resolve paths via the runtime's internal context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigureDomainContext {
    /// The custom domain being configured (e.g., "example.com")
    pub domain: String,

    /// Deployment information from the last deploy
    pub deployment: DeploymentInfo,

    /// Plugin configuration from .moss/config.toml
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

// The `check_setup` context and verdict protocol live in
// [`super::setup`] — re-exported flat by `plugins.rs` alongside these.

/// Context for syndicate capability (POSSE distribution after deployment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyndicateContext {
    /// Absolute path to the project folder being built
    pub project_path: String,

    /// Absolute path to .moss directory
    pub moss_dir: String,

    /// Absolute path to output directory (.moss/site)
    pub output_dir: String,

    /// Project structure information
    pub project_info: ProjectInfo,

    /// List of files deployed (relative to output_dir)
    pub site_files: Vec<String>,

    /// Articles being processed (for syndication)
    #[serde(default)]
    pub articles: Vec<ArticleInfo>,

    /// Deployment result (populated after deployment)
    pub deployment: Option<DeploymentInfo>,

    /// Plugin configuration from .moss/config.toml
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,

    /// Why moss is invoking this hook (ADR-015). The one production caller,
    /// `syndicate_to_platforms`, runs only from the user's Publish click —
    /// always `ManualOne`, never the quiet `Background` default.
    #[serde(default)]
    pub trigger: TriggerContext,
}

#[cfg(test)]
mod syndicate_context_trigger_tests {
    use super::*;

    /// A syndicate hook's own JSON wire (what the plugin actually reads) must
    /// carry the loud `manual_one` value on the run moss really makes — not
    /// silently deserialize into the quiet `Background` default a plugin
    /// would otherwise treat as "no user is watching this".
    #[test]
    fn manual_one_serializes_and_round_trips() {
        let ctx = SyndicateContext {
            project_path: "/site".to_string(),
            moss_dir: "/site/.moss".to_string(),
            output_dir: "/site/.moss/site".to_string(),
            project_info: ProjectInfo {
                total_files: 0,
                homepage_file: None,
                folder_name: None,
                site_name: None,
                lang: "en".into(),
            },
            site_files: Vec::new(),
            articles: Vec::new(),
            deployment: None,
            config: HashMap::new(),
            trigger: TriggerContext::ManualOne,
        };
        let json = serde_json::to_string(&ctx).expect("serialize");
        assert!(json.contains(r#""trigger":"manual_one""#), "got: {json}");

        let round_tripped: SyndicateContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_tripped.trigger, TriggerContext::ManualOne);
    }

    /// A context serialized before this field existed (or any caller that
    /// omits it) must deserialize to the quiet `Background` default, never
    /// fail to parse or silently claim a user is present.
    #[test]
    fn absent_trigger_deserializes_to_background() {
        let json = r#"{
            "project_path": "/site",
            "moss_dir": "/site/.moss",
            "output_dir": "/site/.moss/site",
            "project_info": { "total_files": 0, "homepage_file": null, "site_name": null, "lang": "en" },
            "site_files": [],
            "deployment": null
        }"#;
        let ctx: SyndicateContext = serde_json::from_str(json).expect("deserialize");
        assert_eq!(ctx.trigger, TriggerContext::Background);
    }
}

/// Project information passed to plugins
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProjectInfo {
    /// Total number of files
    pub total_files: usize,

    /// Homepage file path (if detected)
    pub homepage_file: Option<String>,

    /// Root folder basename (e.g. "My Site"), if resolvable from the project path.
    /// Plugins that generate a folder home name it self-named (`<folder_name>.md`)
    /// with a `home: true` marker, matching moss's folder-home convention.
    pub folder_name: Option<String>,

    /// Site name auto-resolved from homepage title or folder name.
    /// Distinct from `[site] name` in .moss/config.toml, the user's override.
    pub site_name: Option<String>,

    /// The site's BCP-47 language code (e.g. "en", "zh-hans", "zh-hant") — the same
    /// ladder the pages take: `[site] lang`, then a homepage `lang:`, then content
    /// detection, then the system language. Not detection alone (2026-09-01).
    /// Used by plugins for i18n of UI copy (comment forms, subscribe buttons, etc.).
    pub lang: String,
}

impl ProjectInfo {
    /// Build ProjectInfo from a scanned ProjectStructure and the resolved vault root.
    ///
    /// Single constructor — resolves the site language, resolves site name from
    /// homepage, populates all fields. `folder_name` is read off the root rather than
    /// re-derived: it is what tells a generator plugin to write the self-named
    /// `<folder>.md` home instead of a bare `index.md`.
    pub fn from_structure(
        ps: &crate::types::content::ProjectStructure,
        root: &crate::vault::paths::VaultRoot,
    ) -> Self {
        Self {
            total_files: ps.total_files,
            homepage_file: ps.homepage_file.clone(),
            folder_name: root.name_opt().map(str::to_string),
            site_name: crate::build::resolve_site_name(root, ps.homepage_file.as_deref()),
            // The SAME ladder the render path and the slot path take. Until
            // 2026-09-01 this was content sampling alone, so a site with an
            // explicit `[site] lang` handed its plugins the detected language
            // and its pages the declared one — one question, two answers.
            lang: crate::i18n::detect::resolve_site_default_lang(
                crate::build::site_config::get_site_lang(&ps.root_path)
                    .ok()
                    .flatten()
                    .as_deref(),
                ps.homepage_file.as_deref(),
                &ps.markdown_files,
                &ps.root_path,
            ),
        }
    }
}

// DnsRecord/DnsTarget moved to `crate::config::deployment` (2026-08-27,
// M6a B3): they are deployment-state vocabulary stored in state.toml; plugins
// produce them, the deployment record owns them. Re-exported so
// `plugins::types::DnsTarget` paths stay valid; DeployAddress joined them for
// the same reason.
pub use crate::config::deployment::{AddressKind, DeployAddress, DnsRecord, DnsTarget};

/// Deployment information passed to syndication plugins
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DeploymentInfo {
    /// Deployment method used (github-pages, netlify, etc.)
    pub method: String,

    /// Public URL where site is deployed
    pub url: String,

    /// Deployment timestamp
    pub deployed_at: String,

    /// Additional deployment metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// DNS target for custom domain configuration
    /// Provided by deploy plugins to configure DNS records
    #[serde(default)]
    pub dns_target: Option<DnsTarget>,

    /// Every other way to reach what was just published — `url` above stays
    /// the one canonical address. See [`DeployAddress`].
    #[serde(default)]
    pub addresses: Vec<DeployAddress>,
}

/// Result of a deployment operation - discriminated union for frontend type safety
///
/// Design principle: a hook describes its outcome as data in HookResult.toast;
/// moss owns every status surface and decides how (and whether) it renders.
/// DeployResult only handles infrastructure-level concerns (no plugin installed).
///
/// Uses `status` field as the discriminator for TypeScript pattern matching:
/// - "completed": Plugin executed - check HookResult for details and toast
/// - "no_plugin": No deploy plugin configured (infrastructure concern)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "status")]
pub enum DeployResult {
    /// Plugin completed execution - check HookResult for success/failure and toast
    #[serde(rename = "completed")]
    Completed {
        /// The hook result from the plugin (contains success, message, toast, deployment)
        result: HookResult,
    },
    /// No deploy plugin configured (infrastructure concern, not a plugin failure)
    #[serde(rename = "no_plugin")]
    NoPlugin {
        /// Informational message about missing plugin
        message: String,
    },
}

/// Plugin instance - loaded and ready to execute
///
/// Plugins are always active once discovered. Whether a plugin is actually
/// used for a given hook is determined by:
/// 1. Hooks config in .moss/config.toml (e.g., `[hooks] deploy = "github"`)
/// 2. Auto-activation for single-capability hooks (if only one plugin has the capability)
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Plugin metadata from manifest
    pub manifest: PluginManifest,

    /// Absolute path to plugin directory
    /// Used to load plugin's JavaScript entry file and resolve icon paths
    pub path: PathBuf,
}

impl Plugin {
    /// Get absolute path to plugin icon using convention-over-configuration
    ///
    /// Resolution order:
    /// 1. Check manifest.icon field (if specified)
    /// 2. Fall back to convention-based paths: icon.svg, icon.png, logo.svg, logo.png
    ///
    /// Returns None if no icon file is found
    pub fn get_icon_path(&self) -> Option<PathBuf> {
        icon_path_in(&self.path, self.manifest.icon.as_deref())
    }
}

/// Resolve a plugin icon inside `dir` using convention-over-configuration.
///
/// Shared by `Plugin::get_icon_path` and the registry's installed-icon
/// lookup so the two can never drift on the convention list.
pub fn icon_path_in(dir: &std::path::Path, manifest_icon: Option<&str>) -> Option<PathBuf> {
    // Check manifest-specified icon first
    if let Some(icon) = manifest_icon {
        let path = dir.join(icon);
        if path.exists() {
            return Some(path);
        }
    }

    // Convention-based fallbacks
    for filename in &["icon.svg", "icon.png", "logo.svg", "logo.png"] {
        let path = dir.join(filename);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Toast outcome determines the visual style of the notification
/// - "success": Green/positive - operation completed successfully
/// - "info": Neutral/informational - nothing to do, or skipped
/// - "error": Red/negative - operation failed
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToastOutcome {
    Success,
    Info,
    Error,
}

/// Outcome notification described by a hook result
///
/// Data about what happened, not a rendering instruction: moss maps `outcome`
/// to its own toast severity, timing, and suppression rules (a surface already
/// showing the outcome may swallow it entirely). Rust also populates this on
/// catastrophic failures where no hook ran.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Toast {
    /// Visual style of the toast
    pub outcome: ToastOutcome,
    /// Short display text (e.g., "🟢 Live!", "No changes to deploy")
    pub title: String,
    /// Optional clickable URL (e.g., the deployed site URL)
    pub url: Option<String>,
}

/// Hook execution result
///
/// Design principle: completion and outcome UX travel in the same return value.
/// - `success`: Whether the operation succeeded (for flow control)
/// - `message`: Detailed message for logs/debugging
/// - `toast`: Outcome described as data; moss controls the rendering
/// - `deployment`: Deployment info (for deploy hooks)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HookResult {
    /// Whether the hook executed successfully
    pub success: bool,

    /// Optional message (error or info) for logs/debugging
    pub message: Option<String>,

    /// Outcome notification for moss to present (moss controls rendering);
    /// None = deliberate silence
    #[serde(default)]
    pub toast: Option<Toast>,

    /// Modified context (plugins can update context for next plugin)
    pub context: Option<PluginContext>,

    /// Deployment result (for publisher plugins using on_deploy hook)
    /// Contains the deployed URL and metadata after successful deployment
    #[serde(default)]
    pub deployment: Option<DeploymentInfo>,

    /// Setup verdict (for the optional `check_setup` hook)
    #[serde(default)]
    pub setup: Option<SetupVerdict>,
}

/// Plugin hook progress payload — carried by `MossEvent::PluginProgress`.
///
/// Fields mirror the legacy inline JSON shape that frontends already consume.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct PluginProgressEvent {
    /// Display name of the plugin emitting progress (e.g. `"Enhance"`).
    pub plugin_name: String,
    /// Hook the progress relates to (e.g. `"enhance"`, `"process"`,
    /// `"deploy"`, `"syndicate"`).
    pub hook_name: String,
    /// Free-form phase identifier: `"collecting"`, `"running"`, `"done"`, etc.
    pub phase: String,
    /// Current item index. `0` when total is `0`.
    pub current: u32,
    /// Total items. `0` when progress is indeterminate.
    pub total: u32,
    /// Optional human-readable status message.
    pub message: Option<String>,
    /// `true` on the final emit for this hook.
    pub completed: bool,
}

/// A plugin's PROPOSED advisory (Step 3 Phase 5, §8 + R13) — pre-clamp.
///
/// Structurally identical to [`crate::advisory::Advisory`] but a DISTINCT type
/// so a plugin can never hand moss a *final* advisory across the IPC. A plugin
/// emits a `PluginAdvisory` (carried on `PluginTaskLifecycle::Succeeded`/
/// `Failed`); moss passes it through the severity gavel
/// [`clamp_plugin_advisory`] — the ONLY constructor of a plugin-origin
/// `Advisory`. This is what stops a plugin from popping the panel at will: it
/// proposes meaning (`scope`/`item`/`what`/`action`) and a *requested*
/// severity, but moss owns the verdict.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PluginAdvisory {
    /// Which axis of the system this advisory is about (passed through).
    pub scope: crate::advisory::Scope,
    /// The severity the plugin REQUESTS. moss clamps it (R13).
    pub severity: crate::advisory::Severity,
    /// The item this advisory is about — usually a filename (passed through).
    pub item: Option<String>,
    /// What happened (passed through).
    pub what: String,
    /// The recovery affordance the plugin proposes (passed through). Whether it
    /// is `Action::None` is the gavel's deciding input for a `Blocking`
    /// proposal.
    pub action: crate::advisory::Action,
}

/// The severity gavel (Step 3 Phase 5, §8 + R13) — the SOLE constructor of a
/// plugin-origin [`crate::advisory::Advisory`].
///
/// moss reserves the panel-popping power for core + actionable-blocking,
/// exactly as for Deploy. A plugin's `Blocking` clears the R12 surfacing bar
/// (`Action ≠ None` AND `severity == Blocking`) ONLY if it carries a real
/// recovery affordance; a `Blocking` with `Action::None` is clamped down to a
/// quiet `NeedsAction` hairline dot (it can't auto-pop the panel). Non-blocking
/// severities (`ShippedDegraded`/`NeedsAction`) are not the gavel's business
/// and pass through unchanged.
///
/// Every other field (`scope`/`item`/`what`/`action`) is passed through
/// verbatim — the plugin owns meaning, moss owns the verdict. `item` gets one
/// check first: it is supposed to be the site-relative path of the file the
/// advisory is about (same contract as `Advisory::item`), and the frontend's
/// click-to-open resolves it by joining it onto the open folder. A plugin
/// proposing an absolute path or one that escapes the site root via `..`
/// could point that click anywhere on disk, so such an item is dropped to
/// `None` rather than trusted — a build-wide advisory with no item is always
/// a safe fallback.
pub fn clamp_plugin_advisory(p: PluginAdvisory) -> crate::advisory::Advisory {
    use crate::advisory::{is_site_relative, Action, Severity};
    let severity = match (&p.severity, &p.action) {
        // R13: a plugin Blocking without an actionable affordance can't pop the
        // panel — clamp to the hairline dot.
        (Severity::Blocking, Action::None) => Severity::NeedsAction,
        (s, _) => s.clone(),
    };
    let item = p.item.filter(|item| is_site_relative(item));
    crate::advisory::Advisory {
        scope: p.scope,
        severity,
        item,
        what: p.what,
        action: p.action,
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
