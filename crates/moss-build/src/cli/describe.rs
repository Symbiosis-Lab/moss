//! `moss describe` CLI subcommand. Emits the moss contract surface in
//! human-readable or JSON form. Both binaries call it; every input is already
//! in moss-core or moss-build, and the one thing that is not — which binary
//! is answering — comes in as `binary_version`.

use moss_core::contract::describe::{
    DescribePayload, ManifestFieldInfo, PluginHookInfo, SlotInfo,
};
use moss_core::contract::tokens::load_tokens;

use super::commands::cli_commands;

pub mod css_query;

/// Returns an exit code.
///
/// `binary_version` is the caller's `env!("CARGO_PKG_VERSION")`: the number has
/// to describe the binary that printed it, and an `env!` here would expand to
/// moss-build's. See `DescribePayload::with_binary_version`.
pub fn run(args: &[String], binary_version: &'static str) -> i32 {
    // `--css <selector>` answers a different question from the contract dump:
    // not "what vocabulary exists" but "what defaults am I overriding".
    // Handled first because it needs none of the payload below.
    if let Some(i) = args.iter().position(|a| a == "--css") {
        return match args.get(i + 1) {
            Some(needle) => css_query::run(needle),
            None => {
                eprintln!("error: --css requires a selector, e.g. `moss describe --css .moss-hero`");
                1
            }
        };
    }

    let json = args.iter().any(|a| a == "--json");

    let tokens = match load_tokens() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {}", e);
            return 1;
        }
    };
    let payload = DescribePayload::new(&tokens)
        .with_binary_version(binary_version)
        .with_plugin_contract(plugin_hooks(), manifest_fields(), slots(), cli_commands());

    if json {
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => {
                println!("{}", s);
                0
            }
            Err(e) => {
                eprintln!("error: {}", e);
                1
            }
        }
    } else {
        print_human(&payload);
        0
    }
}

/// Plugin hook table — one entry per `Capability` variant in
/// `plugins/types.rs`.
///
/// Arity rules:
/// - "single": at most one plugin may be active for this hook per project
///   (hooks where two plugins doing the same thing would be redundant or
///   conflict — generate, deploy).
/// - "multiple": many plugins may coexist for this hook (process, enhance,
///   syndicate, import are additive / independent).
fn plugin_hooks() -> Vec<PluginHookInfo> {
    vec![
        PluginHookInfo {
            name: "process",
            description: "Pre-process source files before generation (e.g. download remote assets, transform markdown).",
            arity: "multiple",
            context: "ProcessContext",
        },
        PluginHookInfo {
            name: "deploy",
            description: "Deploy the built site to a hosting platform (e.g. GitHub Pages, Netlify).",
            arity: "single",
            context: "DeployContext",
        },
        PluginHookInfo {
            name: "syndicate",
            description: "POSSE-distribute published content after deployment (e.g. cross-post to Matters, RSS).",
            arity: "multiple",
            context: "SyndicateContext",
        },
        PluginHookInfo {
            name: "import",
            description: "Import content from an external source into the project folder (e.g. Matters profile, RSS feed).",
            arity: "multiple",
            // Intentionally shares ProcessContext with the `process` hook: the import
            // path dispatches through the same process pipeline after fetching content.
            context: "ProcessContext",
        },
    ]
}

/// Manifest field table — mirrors `PluginManifest` in
/// `plugins/types.rs` field by field.
fn manifest_fields() -> Vec<ManifestFieldInfo> {
    vec![
        ManifestFieldInfo {
            name: "name",
            r#type: "string",
            required: true,
            description: "Plugin identifier (e.g. \"matters\"). Must match the plugin directory name.",
        },
        ManifestFieldInfo {
            name: "version",
            r#type: "string",
            required: true,
            description: "Plugin version in semver format (e.g. \"1.0.0\").",
        },
        ManifestFieldInfo {
            name: "entry",
            r#type: "string",
            required: true,
            description: "Entry point JavaScript file path, relative to the plugin directory.",
        },
        ManifestFieldInfo {
            name: "capabilities",
            r#type: "string[]",
            required: false,
            description: "Hook names this plugin implements (e.g. [\"process\", \"syndicate\"]). Each must be a valid hook name. An empty or omitted capabilities field is valid (plugin implements no hooks).",
        },
        ManifestFieldInfo {
            name: "description",
            r#type: "string",
            required: false,
            description: "Human-readable description of what the plugin does.",
        },
        ManifestFieldInfo {
            name: "author",
            r#type: "string",
            required: false,
            description: "Plugin author name or contact.",
        },
        ManifestFieldInfo {
            name: "global_name",
            r#type: "string",
            required: false,
            description: "Global JavaScript variable name the plugin exports. Defaults to PascalCase(name) + \"Plugin\".",
        },
        ManifestFieldInfo {
            name: "display_name",
            r#type: "string",
            required: false,
            description: "Human-readable name shown in the moss Settings UI section title (e.g. \"Comments\").",
        },
        ManifestFieldInfo {
            name: "icon",
            r#type: "string",
            required: false,
            description: "Path to the plugin icon file, relative to the plugin directory. Falls back to icon.svg / icon.png / logo.svg / logo.png.",
        },
        ManifestFieldInfo {
            name: "domain",
            r#type: "string",
            required: false,
            description: "Primary domain for cookie access (e.g. \"matters.town\"). Required for plugins using cookie-based authentication.",
        },
        ManifestFieldInfo {
            name: "domains",
            r#type: "string[]",
            required: false,
            description: "Full set of domains the plugin operates on (e.g. prod + staging). Used for scope-wide cookie clearing on force-fresh login.",
        },
        ManifestFieldInfo {
            name: "config",
            r#type: "object",
            required: false,
            description: "Plugin-specific configuration key-value pairs (e.g. login_url, api_endpoint). Merged with .moss/config.toml at runtime.",
        },
        ManifestFieldInfo {
            name: "config_schema",
            r#type: "object",
            required: false,
            description: "Legacy alias: field name -> type string, folded at parse into the contribution's setup.settings[]. New manifests declare settings[] directly.",
        },
        ManifestFieldInfo {
            name: "config_labels",
            r#type: "object",
            required: false,
            description: "Legacy alias: field name -> label, folded into settings[].label. A dotted \"<field>.<value>\" key labels one enum option.",
        },
        ManifestFieldInfo {
            name: "config_descriptions",
            r#type: "object",
            required: false,
            description: "Legacy alias: field name -> help text, folded into settings[].description.",
        },
        ManifestFieldInfo {
            name: "config_options",
            r#type: "object",
            required: false,
            description: "Legacy alias: field name -> allowed values, folded into settings[].options on a string field.",
        },
        ManifestFieldInfo {
            name: "config_placeholders",
            r#type: "object",
            required: false,
            description: "Legacy alias: field name -> placeholder text, folded into settings[].placeholder.",
        },
        ManifestFieldInfo {
            name: "contributes",
            r#type: "object",
            required: false,
            description: "Contributions (deploy targets, frontmatter fields, embed renderers, job descriptors, stack declarations), VS Code style. Each contribution may carry a setup block — needs (host-checked preconditions like \"stack\"), settings (fields moss draws, the Field vocabulary), credentials, and check (the plugin implements the check_setup hook). A stack contribution carries its own artifact pin (id, version, sources) instead of a setup block.",
        },
        ManifestFieldInfo {
            name: "min_moss_version",
            r#type: "string",
            required: false,
            description: "Minimum moss version this plugin supports (semver). The registry client checks it at install and load; older moss ignores the field.",
        },
        ManifestFieldInfo {
            name: "repository",
            r#type: "string",
            required: false,
            description: "Source repository URL. Display and provenance only.",
        },
        ManifestFieldInfo {
            name: "homepage",
            r#type: "string",
            required: false,
            description: "Homepage URL. Display only.",
        },
        ManifestFieldInfo {
            name: "requires_stack",
            r#type: "boolean",
            required: false,
            description: "Legacy alias: parses as needs: [\"stack\"] on every contribution. New manifests declare the need inside the contribution's setup block.",
        },
        ManifestFieldInfo {
            name: "requires",
            r#type: "array",
            required: false,
            description: "Declared host-capability requirements, named per binary: \"execute_binary:<basename>\" grants exactly one executable (e.g. \"execute_binary:git\"). The bare \"execute_binary\" blanket is a deprecated alias that still grants every binary, with a warning per run.",
        },
    ]
}

/// Slot table, derived from the `Slot` enum in `build/slots.rs`.
///
/// It used to be a third hand-written copy of the same seven strings — one here,
/// one as `SLOT_NAMES` in the enhance module, one as the enum — each with a
/// comment saying it mirrored the others. `Slot::ALL` is the list now, and
/// `Slot::position()` carries this prose, so a slot cannot be documented here
/// and unknown to the injector.
fn slots() -> Vec<SlotInfo> {
    crate::build::slots::Slot::ALL
        .into_iter()
        .map(|slot| SlotInfo {
            name: slot.as_str(),
            position: slot.position(),
            authorable: slot.is_authorable(),
        })
        .collect()
}

fn print_human(payload: &DescribePayload) {
    println!("moss describe");
    println!("  describe_schema_version: {}", payload.describe_schema_version);
    println!("  moss_html_version: {}", payload.moss_html_version);
    println!("  moss_binary_version: {}", payload.moss_binary_version);
    println!();
    println!("Tokens:");
    for (group, tokens) in &payload.tokens {
        println!("  {}", group);
        for t in tokens {
            println!("    --{}: {}", t.name, t.value);
        }
    }
    println!();
    println!("Components ({}):", payload.components.len());
    for c in &payload.components {
        let mut suffix = String::new();
        if c.status == "retired" {
            suffix.push_str(" [retired]");
        }
        if c.authorable {
            suffix.push_str(" [shortcode]");
        }
        println!("  .{} ({}){}", c.class, c.kind, suffix);
    }
    println!();
    // Custom properties and scope attributes belong in the CHEAP form, not just
    // `--json`. They are the theming API in practice — the two most customized
    // moss sites set six of the former and zero design tokens between them — and
    // the whole complaint this surface answers is that the thorough form is too
    // much context to read. Printing them only in the 116 KB payload would have
    // left `moss describe` showing exactly the surface that was already there.
    println!("Custom properties ({}) — set on a component or a scope, not `:root`:", payload.custom_properties.len());
    for p in &payload.custom_properties {
        println!("  {} on .{} (default: {})", p.name, p.owner, p.default);
    }
    println!();
    println!("Scope attributes ({}) — what to scope a structural override to:", payload.scope_attributes.len());
    for a in &payload.scope_attributes {
        let vals = if a.values.is_empty() {
            String::new()
        } else {
            format!(" [{}]", a.values.join("|"))
        };
        println!("  {}[{}]{}", a.selector, a.name, vals);
    }
    println!();
    let public_fm: Vec<_> = payload.frontmatter.iter().filter(|f| !f.skip_schema).collect();
    println!("Frontmatter fields ({} public, {} internal):", public_fm.len(), payload.frontmatter.len() - public_fm.len());
    for f in &public_fm {
        let enum_hint = f.enum_values
            .map(|vals| format!(" [{}]", vals.join("|")))
            .unwrap_or_default();
        println!("  {}: {}{}", f.name, f.field_type, enum_hint);
    }
    println!();
    println!("Plugin hooks ({}):", payload.plugin_hooks.len());
    for h in &payload.plugin_hooks {
        println!("  {} ({}) — {}", h.name, h.arity, h.description);
    }
    println!();
    println!("Template slots ({}):", payload.slots.len());
    for s in &payload.slots {
        let auth = if s.authorable { " [authorable]" } else { "" };
        println!("  {}{} — {}", s.name, auth, s.position);
    }
    println!();
    println!("CLI commands ({}):", payload.cli_commands.len());
    for cmd in &payload.cli_commands {
        println!("  moss {} {} — {}", cmd.name, cmd.args, cmd.description);
    }
    println!();
    // No URL here. This used to print landing.mosspub.com/contract/v1/reference.md,
    // which 404s — and any URL would be a second source of truth that can
    // go stale between releases, which is the whole reason this command exists.
    // Point at the flags of this same binary instead.
    println!("Everything above in full: moss describe --json");
    println!("The CSS behind a class or property: moss describe --css <selector>");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `moss describe` manifest table mirrors `PluginManifest` field by
    /// field (a plugin-author discovery contract). Guard the registry fields
    /// against silent drift — they were added to the struct in the plugin-
    /// registry work but the mirror is enforced only by convention.
    #[test]
    fn manifest_fields_include_registry_fields() {
        let names: Vec<&str> = manifest_fields().iter().map(|f| f.name).collect();
        for expected in ["min_moss_version", "repository", "homepage", "requires"] {
            assert!(
                names.contains(&expected),
                "manifest_fields() is missing '{expected}' — it drifted from PluginManifest in plugins/types.rs"
            );
        }
    }
}
