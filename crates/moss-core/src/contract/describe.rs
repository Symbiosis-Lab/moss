//! Serializable payload for `moss describe --json`.
//!
//! The JSON shape is itself a contract — version it independently of
//! `moss_html_version`. Bumping `describe_schema_version` is required for
//! any breaking change to the JSON envelope.

use serde::Serialize;
use std::collections::BTreeMap;

use crate::ast::shortcode::ShortcodeKind;
use crate::contract::components::{COMPONENTS, Status};
use crate::contract::frontmatter::{FrontmatterFieldJson, frontmatter_fields};
use crate::contract::tokens::Tokens;

pub const DESCRIBE_SCHEMA_VERSION: u32 = 6;
pub const MOSS_HTML_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct DescribePayload<'a> {
    pub describe_schema_version: u32,
    pub moss_html_version: u32,
    pub moss_binary_version: &'static str,
    pub tokens: BTreeMap<&'a str, Vec<TokenJson<'a>>>,
    pub components: Vec<ComponentJson>,
    /// Escape-hatch custom properties: the `var(--moss-*, fallback)` hooks
    /// moss's stylesheets read but never declare. Schema v6 added this — the
    /// theming API both flagship sites actually used, previously undiscoverable.
    pub custom_properties: Vec<CustomPropJson>,
    /// Structural `data-*` attributes on elements that carry no `moss-*` class
    /// — `<body data-page>`, `<html data-theme>`. Schema v6.
    pub scope_attributes: Vec<ScopeAttrJson>,
    pub frontmatter: Vec<FrontmatterFieldJson>,
    /// Every language code that makes a directory or filename suffix a
    /// language edition (`zh-hant/about.md`, `about.zh-hans.md`).
    ///
    /// Additive, so no `describe_schema_version` bump: the envelope's rule
    /// requires one for breaking changes, and a new key breaks no reader.
    ///
    /// Here because the failure it prevents is silent. An unrecognized
    /// directory name is not an error — the tree is treated as ordinary
    /// content — so an agent that invents `english/` gets a build that
    /// succeeds, a page that publishes, and no switcher, with nothing
    /// anywhere saying why. The allowlist has to be readable *before* the
    /// directory is created, and this is the only place it is published.
    pub languages: Vec<&'static str>,
    /// Plugin hook contract: each capability moss supports, with arity and context type.
    pub plugin_hooks: Vec<PluginHookInfo>,
    /// Plugin manifest fields: each field in PluginManifest, with type and required flag.
    pub manifest_fields: Vec<ManifestFieldInfo>,
    /// Template injection slots: each named slot in the build pipeline.
    pub slots: Vec<SlotInfo>,
    /// CLI commands: each subcommand moss exposes.
    // hand-maintained: keep in sync with run_mode.rs
    pub cli_commands: Vec<CliCommandInfo>,
}

/// Plugin hook entry emitted in `plugin_hooks`.
///
/// Describes one capability a plugin may implement. Populated by
/// `src-tauri/src/describe.rs` from the Tauri-layer `Capability` enum.
#[derive(Serialize)]
pub struct PluginHookInfo {
    /// Lowercase hook name (e.g. "process"). Matches the JS function name.
    pub name: &'static str,
    /// One-line description of what this hook does.
    pub description: &'static str,
    /// "single" if at most one plugin may register this hook; "multiple" if many may.
    pub arity: &'static str,
    /// The name of the context struct passed to the hook function.
    pub context: &'static str,
}

/// Plugin manifest field entry emitted in `manifest_fields`.
///
/// Describes one field of `PluginManifest`. Populated by
/// `src-tauri/src/describe.rs` from the Rust struct definition.
#[derive(Serialize)]
pub struct ManifestFieldInfo {
    /// Field name as it appears in the JSON manifest (snake_case).
    pub name: &'static str,
    /// JSON type or Rust-type description (e.g. "string", "string[]", "object").
    pub r#type: &'static str,
    /// Whether this field must be present in a valid manifest.
    pub required: bool,
    /// One-line description.
    pub description: &'static str,
}

/// Template slot entry emitted in `slots`.
///
/// Describes one named injection point in the moss HTML templates. Populated
/// by `src-tauri/src/describe.rs` from `SLOT_NAMES` and the `Slot` enum.
#[derive(Serialize)]
pub struct SlotInfo {
    /// Slot name (e.g. "head-end"). Matches the `<!-- slot:NAME -->` marker.
    pub name: &'static str,
    /// Human-readable description of the slot's position in the page.
    pub position: &'static str,
    /// Whether markdown authors may target this slot via the `slot:` frontmatter field.
    pub authorable: bool,
}

/// CLI command entry emitted in `cli_commands`.
///
/// Describes one moss CLI subcommand.
// hand-maintained: keep in sync with run_mode.rs
#[derive(Serialize)]
pub struct CliCommandInfo {
    /// Subcommand name (e.g. "build").
    pub name: &'static str,
    /// Argument signature (e.g. "<folder> [--serve] [--watch] [--no-plugins]").
    pub args: &'static str,
    /// One-line description.
    pub description: &'static str,
}

#[derive(Serialize)]
pub struct TokenJson<'a> {
    pub name: &'a str,
    pub value: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark_value: Option<&'a str>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_hint: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
}

#[derive(Serialize)]
pub struct ComponentJson {
    pub class: &'static str,
    pub kind: &'static str,
    pub parent: &'static str,
    pub data_attrs: Vec<DataAttrJson>,
    pub example_html: &'static str,
    pub example_markdown: &'static str,
    pub status: &'static str,
    pub since: &'static str,
    pub description: &'static str,
    /// True iff this class is the root class of an authorable shortcode
    /// (i.e. it appears in `ShortcodeKind::all().map(|k| k.root_class())`).
    /// Agents can use this flag to distinguish the author-facing shortcodes
    /// from the broader theme vocabulary. Deliberately not stated as a count:
    /// this said "6" while `ShortcodeKind` had 7, and a number here is a second
    /// source of truth for something the flag itself already answers.
    pub authorable: bool,
}

#[derive(Serialize)]
pub struct DataAttrJson {
    pub name: &'static str,
    pub values: &'static [&'static str],
    pub default: &'static str,
    pub description: &'static str,
}

/// A structural attribute on a classless element. Top-level for the same reason
/// as [`CustomPropJson`]: it has no component to nest under.
#[derive(Serialize)]
pub struct ScopeAttrJson {
    pub selector: &'static str,
    pub name: &'static str,
    pub values: &'static [&'static str],
    pub description: &'static str,
}

/// An escape-hatch custom property, emitted as its own top-level section rather
/// than nested under a component: an agent looking for "how do I change the hero
/// crop" greps for the property name, and several of these are not owned by a
/// single class anyway.
#[derive(Serialize)]
pub struct CustomPropJson {
    pub name: &'static str,
    pub owner: &'static str,
    pub default: &'static str,
    pub description: &'static str,
}

impl<'a> DescribePayload<'a> {
    pub fn new(tokens: &'a Tokens) -> Self {
        let mut tokens_map: BTreeMap<&str, Vec<TokenJson>> = BTreeMap::new();
        for group in &tokens.groups {
            let entries: Vec<TokenJson> = group
                .entries
                .iter()
                .map(|t| TokenJson {
                    name: &t.name,
                    value: &t.value,
                    dark_value: t.dark_value.as_deref(),
                    type_hint: t.type_hint.as_deref(),
                    description: t.description.as_deref(),
                })
                .collect();
            tokens_map.insert(&group.name, entries);
        }

        let authorable: std::collections::HashSet<&'static str> =
            ShortcodeKind::all().map(|k| k.root_class()).collect();

        let components: Vec<ComponentJson> = COMPONENTS
            .iter()
            .filter(|c| c.is_public())
            .map(|c| ComponentJson {
                class: c.class,
                kind: c.kind,
                parent: c.parent,
                data_attrs: c
                    .data_attrs
                    .iter()
                    .map(|a| DataAttrJson {
                        name: a.name,
                        values: a.values,
                        default: a.default,
                        description: a.description,
                    })
                    .collect(),
                example_html: c.example_html,
                example_markdown: c.example_markdown,
                status: match c.status {
                    Status::Confirmed => "confirmed",
                    Status::Emerging => "emerging",
                    Status::Retired => "retired",
                },
                since: c.since,
                description: c.description,
                authorable: authorable.contains(c.class),
            })
            .collect();

        DescribePayload {
            describe_schema_version: DESCRIBE_SCHEMA_VERSION,
            moss_html_version: MOSS_HTML_VERSION,
            moss_binary_version: env!("CARGO_PKG_VERSION"),
            tokens: tokens_map,
            components,
            custom_properties: crate::contract::custom_props::CUSTOM_PROPS
                .iter()
                .map(|p| CustomPropJson {
                    name: p.name,
                    owner: p.owner,
                    default: p.default,
                    description: p.description,
                })
                .collect(),
            scope_attributes: crate::contract::custom_props::SCOPE_ATTRS
                .iter()
                .map(|a| ScopeAttrJson {
                    selector: a.selector,
                    name: a.name,
                    values: a.values,
                    description: a.description,
                })
                .collect(),
            frontmatter: frontmatter_fields(),
            languages: crate::home::known_language_codes(),
            // Populated by the Tauri layer (src-tauri/src/describe.rs) which
            // has access to the Tauri-layer plugin types. Callers using
            // DescribePayload::new() directly (e.g. moss-core unit tests) get
            // empty vecs here; the CLI path fills them via with_plugin_contract().
            plugin_hooks: Vec::new(),
            manifest_fields: Vec::new(),
            slots: Vec::new(),
            cli_commands: Vec::new(),
        }
    }

    /// Builder method: report the version of the moss binary that is answering,
    /// rather than the version of this crate.
    ///
    /// `env!("CARGO_PKG_VERSION")` expands where it is *written*, so the default
    /// set in [`DescribePayload::new`] is moss-core's version — a different
    /// number from the app's, on its own release cadence. `describe --json`
    /// therefore reported `0.4.0` while `moss --version` reported `0.8.0`,
    /// under a field named `moss_binary_version`. An agent keying a vocabulary
    /// cache on it saw a version that never matched the binary and did not move
    /// when the app was upgraded.
    ///
    /// Only the host crate can answer this, so it has to be passed in.
    pub fn with_binary_version(mut self, version: &'static str) -> Self {
        self.moss_binary_version = version;
        self
    }

    /// Builder method: attach plugin contract data (hooks, manifest fields,
    /// slots, CLI commands). Called by the Tauri-layer describe.rs after
    /// constructing the base payload, since those types live outside moss-core.
    pub fn with_plugin_contract(
        mut self,
        plugin_hooks: Vec<PluginHookInfo>,
        manifest_fields: Vec<ManifestFieldInfo>,
        slots: Vec<SlotInfo>,
        cli_commands: Vec<CliCommandInfo>,
    ) -> Self {
        self.plugin_hooks = plugin_hooks;
        self.manifest_fields = manifest_fields;
        self.slots = slots;
        self.cli_commands = cli_commands;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::tokens::load_tokens;

    #[test]
    fn apply_is_absent_from_public_contract() {
        let tokens = load_tokens().expect("tokens");
        let payload = DescribePayload::new(&tokens);
        assert!(
            payload.components.iter().all(|c| !c.class.starts_with("moss-apply")),
            "apply classes must be demoted from the public contract"
        );
        assert!(
            !payload.components.iter().any(|c| c.class == "moss-apply" && c.authorable),
            ":::apply must not be marked authorable"
        );
    }
}
