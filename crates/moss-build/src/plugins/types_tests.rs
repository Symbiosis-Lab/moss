use super::*;
use std::fs;

// ── The severity gavel (Step 3 Phase 5, §8 + R13) ─────────────────────
//
// A plugin PROPOSES an advisory severity; moss holds the gavel. A plugin's
// `Blocking` clears the same R12 bar as Deploy (Action ≠ None) — otherwise
// it is clamped down to a quiet `NeedsAction` hairline dot. The plugin can
// never hand moss a final `Advisory`; only `clamp_plugin_advisory`
// constructs one from a `PluginAdvisory` proposal.

#[test]
fn plugin_blocking_advisory_clamped_unless_actionable() {
    use crate::advisory::{Action, Scope, Severity};
    // R13: moss holds the severity gavel. A plugin Blocking with Action::None
    // and no consequential intent is clamped to NeedsAction (hairline dot).
    let proposed = PluginAdvisory {
        scope: Scope::Remote,
        severity: Severity::Blocking,
        item: None,
        what: "rate limited".into(),
        action: Action::None,
    };
    let clamped = clamp_plugin_advisory(proposed);
    assert!(matches!(clamped.severity, Severity::NeedsAction));
    // The non-severity fields pass through untouched.
    assert!(matches!(clamped.scope, Scope::Remote));
    assert_eq!(clamped.what, "rate limited");
    assert!(matches!(clamped.action, Action::None));
}

#[test]
fn plugin_blocking_with_action_keeps_blocking() {
    use crate::advisory::{Action, AppOp, Scope, Severity};
    let proposed = PluginAdvisory {
        scope: Scope::Account,
        severity: Severity::Blocking,
        item: None,
        what: "auth expired".into(),
        action: Action::InApp {
            op: AppOp::SignIn,
            args: serde_json::Value::Null,
            label: "Sign in".into(),
        },
    };
    let kept = clamp_plugin_advisory(proposed);
    assert!(matches!(kept.severity, Severity::Blocking));
}

#[test]
fn plugin_non_blocking_severities_pass_through() {
    use crate::advisory::{Action, Scope, Severity};
    // A `ShippedDegraded` / `NeedsAction` proposal is not the gavel's
    // business — it passes through unchanged regardless of action.
    for sev in [Severity::ShippedDegraded, Severity::NeedsAction] {
        let proposed = PluginAdvisory {
            scope: Scope::File,
            severity: sev.clone(),
            item: Some("clip.mov".into()),
            what: "shipped unoptimized".into(),
            action: Action::None,
        };
        let clamped = clamp_plugin_advisory(proposed);
        assert_eq!(clamped.severity, sev);
    }
}

#[test]
fn plugin_item_outside_the_site_root_is_dropped() {
    use crate::advisory::{Action, Scope, Severity};
    // `item` carries the same contract as `Advisory::item`: the frontend's
    // click-to-open resolves it by joining it onto the open folder, so an
    // absolute path or one that escapes via `..` would point that click
    // anywhere on disk. A plugin proposing one is dropped to None rather
    // than trusted.
    for item in ["/etc/passwd", "../../etc/passwd", "posts/../../etc/passwd"] {
        let proposed = PluginAdvisory {
            scope: Scope::File,
            severity: Severity::NeedsAction,
            item: Some(item.to_string()),
            what: "suspicious item".into(),
            action: Action::None,
        };
        let clamped = clamp_plugin_advisory(proposed);
        assert_eq!(clamped.item, None, "{item:?} must be dropped");
    }

    // A real site-relative path — with or without a directory — passes
    // through untouched, same as every other field.
    for item in ["clip.mov", "posts/2026/hello.md"] {
        let proposed = PluginAdvisory {
            scope: Scope::File,
            severity: Severity::NeedsAction,
            item: Some(item.to_string()),
            what: "shipped unoptimized".into(),
            action: Action::None,
        };
        let clamped = clamp_plugin_advisory(proposed);
        assert_eq!(clamped.item.as_deref(), Some(item));
    }
}

#[test]
fn manifest_parses_registry_fields() {
    let json = r#"{
            "name": "example",
            "version": "1.2.0",
            "entry": "main.bundle.js",
            "min_moss_version": "0.8.0",
            "repository": "https://github.com/Symbiosis-Lab/moss-plugins/tree/main/plugins/example",
            "homepage": "https://example.com",
            "requires": ["execute_binary"]
        }"#;
    let m: PluginManifest = serde_json::from_str(json).unwrap();
    assert_eq!(m.min_moss_version.as_deref(), Some("0.8.0"));
    assert_eq!(
        m.repository.as_deref(),
        Some("https://github.com/Symbiosis-Lab/moss-plugins/tree/main/plugins/example")
    );
    assert_eq!(m.homepage.as_deref(), Some("https://example.com"));
    assert_eq!(
        m.requires.as_deref(),
        Some(&["execute_binary".to_string()][..])
    );
}

#[test]
fn manifest_omitting_registry_fields_defaults_to_none() {
    let json = r#"{"name":"x","version":"1.0.0","entry":"main.js"}"#;
    let m: PluginManifest = serde_json::from_str(json).unwrap();
    assert!(m.min_moss_version.is_none());
    assert!(m.repository.is_none());
    assert!(m.homepage.is_none());
    assert!(m.requires.is_none());
}

/// What the settings page reports must be what the gate will admit, so the
/// list is derived from `requires` with the gate's own grammar rather than
/// printed raw: an entry the gate ignores names no binary here either.
#[test]
fn granted_binaries_names_only_what_the_gate_admits() {
    let manifest = |requires: &str| -> PluginManifest {
        serde_json::from_str(&format!(
            r#"{{"name":"x","version":"1.0.0","entry":"main.js","requires":{requires}}}"#
        ))
        .unwrap()
    };

    let named = manifest(r#"["execute_binary:git", "execute_binary:tar"]"#);
    assert_eq!(named.granted_binaries(), vec!["git", "tar"]);
    assert!(!named.grants_any_binary());

    // The blanket token names nothing and grants everything — a caller that
    // read only the list would report "runs nothing" about a plugin allowed
    // to run anything.
    let blanket = manifest(r#"["execute_binary"]"#);
    assert!(blanket.granted_binaries().is_empty());
    assert!(blanket.grants_any_binary());

    // `requires` gates nothing but `execute_binary` today, and an empty
    // basename resolves to no grant in `require_binary_grant`. Neither may
    // reach a surface that claims to say what will run.
    let noise = manifest(r#"["network", "execute_binary:", "keystore"]"#);
    assert!(noise.granted_binaries().is_empty());
    assert!(!noise.grants_any_binary());

    let absent: PluginManifest =
        serde_json::from_str(r#"{"name":"x","version":"1.0.0","entry":"main.js"}"#).unwrap();
    assert!(absent.granted_binaries().is_empty());
    assert!(!absent.grants_any_binary());
}

#[test]
fn test_get_icon_path_with_manifest_icon() {
    // Create temporary plugin directory structure
    let temp_dir = tempfile::tempdir().unwrap();
    let plugin_dir = temp_dir.path();

    // Create icon file
    let icon_path = plugin_dir.join("custom-icon.svg");
    fs::write(&icon_path, "<svg></svg>").unwrap();

    // Create plugin with manifest specifying icon
    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            entry: "main.js".to_string(),
            capabilities: vec![Capability::Syndicate],
            icon: Some("custom-icon.svg".to_string()),
            ..Default::default()
        },
        path: plugin_dir.to_path_buf(),
    };

    // Should return the manifest-specified icon
    let result = plugin.get_icon_path();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), icon_path);
}

#[test]
fn test_get_icon_path_convention_icon_svg() {
    let temp_dir = tempfile::tempdir().unwrap();
    let plugin_dir = temp_dir.path();

    // Create icon.svg (first convention fallback)
    let icon_path = plugin_dir.join("icon.svg");
    fs::write(&icon_path, "<svg></svg>").unwrap();

    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            entry: "main.js".to_string(),
            capabilities: vec![Capability::Syndicate], // No manifest icon
            domain: None,
            ..Default::default()
        },
        path: plugin_dir.to_path_buf(),
    };

    let result = plugin.get_icon_path();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), icon_path);
}

#[test]
fn test_get_icon_path_convention_icon_png() {
    let temp_dir = tempfile::tempdir().unwrap();
    let plugin_dir = temp_dir.path();

    // Create icon.png (second convention fallback)
    let icon_path = plugin_dir.join("icon.png");
    fs::write(&icon_path, b"fake png data").unwrap();

    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            entry: "main.js".to_string(),
            capabilities: vec![Capability::Syndicate],
            ..Default::default()
        },
        path: plugin_dir.to_path_buf(),
    };

    let result = plugin.get_icon_path();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), icon_path);
}

#[test]
fn test_get_icon_path_convention_logo_svg() {
    let temp_dir = tempfile::tempdir().unwrap();
    let plugin_dir = temp_dir.path();

    // Create logo.svg (third convention fallback)
    let icon_path = plugin_dir.join("logo.svg");
    fs::write(&icon_path, "<svg></svg>").unwrap();

    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            entry: "main.js".to_string(),
            capabilities: vec![Capability::Syndicate],
            ..Default::default()
        },
        path: plugin_dir.to_path_buf(),
    };

    let result = plugin.get_icon_path();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), icon_path);
}

#[test]
fn test_get_icon_path_no_icon() {
    let temp_dir = tempfile::tempdir().unwrap();
    let plugin_dir = temp_dir.path();

    // Don't create any icon files

    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            entry: "main.js".to_string(),
            capabilities: vec![Capability::Syndicate],
            ..Default::default()
        },
        path: plugin_dir.to_path_buf(),
    };

    let result = plugin.get_icon_path();
    assert!(result.is_none());
}

#[test]
fn test_get_icon_path_manifest_takes_precedence() {
    let temp_dir = tempfile::tempdir().unwrap();
    let plugin_dir = temp_dir.path();

    // Create both custom icon and convention icon
    let custom_icon = plugin_dir.join("my-icon.svg");
    fs::write(&custom_icon, "<svg>custom</svg>").unwrap();

    let convention_icon = plugin_dir.join("icon.svg");
    fs::write(&convention_icon, "<svg>convention</svg>").unwrap();

    let plugin = Plugin {
        manifest: PluginManifest {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            entry: "main.js".to_string(),
            capabilities: vec![Capability::Syndicate],
            icon: Some("my-icon.svg".to_string()),
            ..Default::default()
        },
        path: plugin_dir.to_path_buf(),
    };

    // Should return custom icon from manifest, not convention
    let result = plugin.get_icon_path();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), custom_icon);
}

// ===========================================================================
// ProjectInfo Tests
// ===========================================================================

#[test]
fn test_project_info_with_files() {
    let info = ProjectInfo {
        total_files: 10,
        homepage_file: Some("index.md".to_string()),
        folder_name: None,
        site_name: Some("My Blog".to_string()),
        lang: "en".into(),
    };

    assert_eq!(info.total_files, 10);
    assert_eq!(info.homepage_file, Some("index.md".to_string()));
    assert_eq!(info.site_name, Some("My Blog".to_string()));
}

#[test]
fn test_project_info_no_homepage() {
    let info = ProjectInfo {
        total_files: 5,
        homepage_file: None,
        folder_name: None,
        site_name: None,
        lang: "en".into(),
    };

    assert!(info.homepage_file.is_none());
    assert!(info.site_name.is_none());
}

#[test]
fn test_project_info_serialization() {
    let info = ProjectInfo {
        total_files: 5,
        homepage_file: Some("index.md".to_string()),
        folder_name: None,
        site_name: Some("Test Site".to_string()),
        lang: "en".into(),
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("total_files"));
    assert!(json.contains("homepage_file"));
    assert!(json.contains("index.md"));
    assert!(json.contains("site_name"));
    assert!(json.contains("Test Site"));
    assert!(!json.contains("project_type"));
    assert!(!json.contains("content_folders"));

    let deserialized: ProjectInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_files, 5);
    assert_eq!(deserialized.homepage_file, Some("index.md".to_string()));
    assert_eq!(deserialized.site_name, Some("Test Site".to_string()));
}

#[test]
fn test_project_info_serialization_without_site_name() {
    let info = ProjectInfo {
        total_files: 3,
        homepage_file: None,
        folder_name: None,
        site_name: None,
        lang: "en".into(),
    };

    let json = serde_json::to_string(&info).unwrap();
    // site_name should still appear in JSON (as null) since it's Option
    let deserialized: ProjectInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_files, 3);
    assert!(deserialized.homepage_file.is_none());
    assert!(deserialized.site_name.is_none());
}

// ===========================================================================
// Capability Tests
// ===========================================================================

#[test]
fn test_capability_hook_names() {
    assert_eq!(Capability::Process.hook_name(), "process");
    assert_eq!(Capability::Deploy.hook_name(), "deploy");
    assert_eq!(Capability::Syndicate.hook_name(), "syndicate");
}

#[test]
fn test_capability_serialization() {
    // Test lowercase serialization
    let cap = Capability::Syndicate;
    let json = serde_json::to_string(&cap).unwrap();
    assert_eq!(json, "\"syndicate\"");

    let deserialized: Capability = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, Capability::Syndicate);
}

#[test]
fn test_capabilities_array_serialization() {
    let caps = vec![Capability::Process, Capability::Syndicate];
    let json = serde_json::to_string(&caps).unwrap();
    assert_eq!(json, "[\"process\",\"syndicate\"]");

    let deserialized: Vec<Capability> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.len(), 2);
    assert_eq!(deserialized[0], Capability::Process);
    assert_eq!(deserialized[1], Capability::Syndicate);
}

#[test]
fn test_plugin_manifest_has_capability() {
    let manifest = PluginManifest {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        entry: "main.js".to_string(),
        capabilities: vec![Capability::Process, Capability::Syndicate],
        ..Default::default()
    };

    assert!(manifest.has_capability(&Capability::Process));
    assert!(manifest.has_capability(&Capability::Syndicate));
    assert!(!manifest.has_capability(&Capability::Deploy));
    assert!(!manifest.has_capability(&Capability::Import));
}

#[test]
fn test_manifest_with_capabilities_json() {
    let json = r#"{
            "name": "super-plugin",
            "version": "1.0.0",
            "entry": "main.bundle.js",
            "capabilities": ["process", "syndicate"]
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.name, "super-plugin");
    assert_eq!(manifest.capabilities.len(), 2);
    assert!(manifest.has_capability(&Capability::Process));
    assert!(manifest.has_capability(&Capability::Syndicate));
}

#[test]
fn test_manifest_empty_capabilities_default() {
    let json = r#"{
            "name": "minimal-plugin",
            "version": "1.0.0",
            "entry": "main.js"
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.capabilities.is_empty());
}

// ===========================================================================
// Manifest UI Metadata Tests (display_name, config_labels, etc.)
// ===========================================================================

#[test]
fn test_manifest_with_ui_metadata_fields() {
    let json = r#"{
            "name": "comment",
            "version": "1.0.0",
            "entry": "main.js",
            "capabilities": ["process"],
            "config_schema": {
                "enabled": "boolean"
            },
            "display_name": "Comments",
            "config_labels": {
                "enabled": "Enable Comments"
            },
            "config_descriptions": {
                "enabled": "Show a comment section on every article page"
            },
            "config_placeholders": {
                "api_key": "Enter your API key here"
            }
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();

    // display_name
    assert_eq!(manifest.display_name, Some("Comments".to_string()));

    // config_labels
    let labels = manifest.config_labels.as_ref().unwrap();
    assert_eq!(labels.get("enabled"), Some(&"Enable Comments".to_string()));

    // config_descriptions
    let descriptions = manifest.config_descriptions.as_ref().unwrap();
    assert_eq!(
        descriptions.get("enabled"),
        Some(&"Show a comment section on every article page".to_string())
    );

    // config_placeholders
    let placeholders = manifest.config_placeholders.as_ref().unwrap();
    assert_eq!(
        placeholders.get("api_key"),
        Some(&"Enter your API key here".to_string())
    );
}

#[test]
fn test_manifest_without_ui_metadata_defaults_to_none() {
    let json = r#"{
            "name": "minimal-plugin",
            "version": "1.0.0",
            "entry": "main.js"
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();

    assert!(manifest.display_name.is_none());
    assert!(manifest.config_labels.is_none());
    assert!(manifest.config_descriptions.is_none());
    assert!(manifest.config_placeholders.is_none());
}

// ===========================================================================
// ConfigureDomainContext Tests
// ===========================================================================

#[test]
fn test_configure_domain_context_serialization() {
    let context = ConfigureDomainContext {
        domain: "example.com".to_string(),
        deployment: DeploymentInfo {
            method: "github-pages".to_string(),
            url: "https://user.github.io/repo".to_string(),
            deployed_at: "2025-01-15T10:30:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
            dns_target: Some(DnsTarget {
                records: vec![DnsRecord {
                    record_type: "A".to_string(),
                    name: "@".to_string(),
                    value: "185.199.108.153".to_string(),
                    ttl: Some(3600),
                }],
            }),
            addresses: Vec::new(),
        },
        config: std::collections::HashMap::new(),
    };

    let json = serde_json::to_string(&context).unwrap();
    assert!(json.contains("example.com"));
    assert!(json.contains("github-pages"));
    assert!(json.contains("185.199.108.153"));
    // Verify path fields are NOT present
    assert!(!json.contains("project_path"));
    assert!(!json.contains("moss_dir"));

    let deserialized: ConfigureDomainContext = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.domain, "example.com");
    assert_eq!(deserialized.deployment.method, "github-pages");
    assert_eq!(deserialized.deployment.url, "https://user.github.io/repo");
    assert!(deserialized.deployment.dns_target.is_some());
}

#[test]
fn test_configure_domain_context_minimal() {
    let context = ConfigureDomainContext {
        domain: "blog.example.com".to_string(),
        deployment: DeploymentInfo {
            method: "netlify".to_string(),
            url: "https://example.netlify.app".to_string(),
            deployed_at: "2025-02-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
            dns_target: None,
            addresses: Vec::new(),
        },
        config: std::collections::HashMap::new(),
    };

    let json = serde_json::to_string(&context).unwrap();
    let deserialized: ConfigureDomainContext = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.domain, "blog.example.com");
    assert_eq!(deserialized.deployment.method, "netlify");
    assert!(deserialized.deployment.dns_target.is_none());
}

#[test]
fn test_manifest_with_contributes_frontmatter() {
    let json = r#"{
            "name": "review",
            "version": "0.2.0",
            "entry": "main.bundle.js",
            "capabilities": ["process"],
            "contributes": {
                "frontmatter": {
                    "fields": {
                        "review_of": {
                            "type": "string",
                            "widget": "text-input",
                            "description": "URL of item being reviewed"
                        },
                        "rating": {
                            "type": "integer",
                            "widget": "number-input",
                            "description": "Your rating (1-5 stars)"
                        }
                    }
                }
            }
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    let contributes = manifest.contributes.expect("contributes should be present");
    let fm = contributes
        .frontmatter
        .expect("frontmatter should be present");
    assert_eq!(fm.fields.len(), 2);

    let review_of = fm.fields.get("review_of").expect("review_of field");
    assert_eq!(review_of.field_type, moss_core::schema::FieldType::String);
    assert_eq!(review_of.widget, Some(moss_core::schema::Widget::TextInput));

    let rating = fm.fields.get("rating").expect("rating field");
    assert_eq!(rating.field_type, moss_core::schema::FieldType::Integer);
    assert_eq!(rating.widget, Some(moss_core::schema::Widget::NumberInput));
}

#[test]
fn test_manifest_without_contributes_defaults_to_none() {
    let json = r#"{
            "name": "minimal",
            "version": "1.0.0",
            "entry": "main.js"
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.contributes.is_none());
}

#[test]
fn test_manifest_with_empty_contributes() {
    let json = r#"{
            "name": "test",
            "version": "1.0.0",
            "entry": "main.js",
            "contributes": {}
        }"#;

    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    let contributes = manifest.contributes.expect("contributes should be present");
    assert!(contributes.frontmatter.is_none());
    assert!(contributes.jobs.is_none());
}

#[test]
fn manifest_parses_contributes_jobs() {
    // §8 + R13: a plugin DECLARES a Job descriptor (verb + amount-noun) in
    // its manifest; moss normalizes the verb and owns the surface. The
    // settled serde shape is the BARE map directly under `jobs` —
    // `ContributedJobs` is `#[serde(transparent)]` over `descriptors`, so
    // the wire shape is `{ "jobs": { "<id>": {verb, noun} } }` (nested),
    // NOT `{ "jobs": { "descriptors": { ... } } }`. This parallels how
    // `contributes.frontmatter.fields` reads (a keyed map) while keeping a
    // named Rust accessor (`jobs.descriptors`).
    let json = r#"{
          "name": "matters", "version": "1.0.0", "entry": "main.js",
          "contributes": { "jobs": { "syndicate": { "verb": "Syndicated", "noun": "posts" } } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).unwrap();
    let jobs = m.contributes.unwrap().jobs.unwrap();
    let d = jobs.descriptors.get("syndicate").unwrap();
    assert_eq!(d.verb, "Syndicated");
    assert_eq!(d.noun, "posts");
}

#[test]
fn contributes_jobs_round_trips_as_a_bare_map() {
    // The settled shape MUST round-trip: serializing a manifest with
    // contributes.jobs produces the bare nested map (no `descriptors` key
    // on the wire), and re-parsing it recovers the descriptor. This pins
    // the §12 open-question decision (transparent newtype) against drift.
    let json = r#"{
          "name": "matters", "version": "1.0.0", "entry": "main.js",
          "contributes": { "jobs": { "syndicate": { "verb": "Syndicated", "noun": "posts" } } }
        }"#;
    let m: PluginManifest = serde_json::from_str(json).unwrap();
    let serialized = serde_json::to_string(&m).unwrap();
    // The wire form carries the bare map, never a `descriptors` wrapper key.
    assert!(
        serialized.contains(r#""jobs":{"syndicate":{"verb":"Syndicated","noun":"posts"}}"#),
        "expected bare nested map on the wire, got: {serialized}"
    );
    assert!(!serialized.contains("descriptors"), "got: {serialized}");

    let reparsed: PluginManifest = serde_json::from_str(&serialized).unwrap();
    let jobs = reparsed.contributes.unwrap().jobs.unwrap();
    let d = jobs.descriptors.get("syndicate").unwrap();
    assert_eq!(d.verb, "Syndicated");
    assert_eq!(d.noun, "posts");
}

// ── PluginHook / TriggerContext / Capability::Import ──────────────────
//
// Foundational types for PanelTask. These tests pin the
// serde wire format (JSON-side spelling) so the closed enums stay
// stable across the router (T1), the matters manifest, and bindings.ts.

#[test]
fn capability_import_round_trips_as_lowercase_string() {
    let cap = Capability::Import;
    let json = serde_json::to_string(&cap).unwrap();
    assert_eq!(json, "\"import\"");

    let parsed: Capability = serde_json::from_str("\"import\"").unwrap();
    assert_eq!(parsed, Capability::Import);
    assert_eq!(parsed.hook_name(), "import");
}

#[test]
fn plugin_hook_serializes_all_variants_lowercase() {
    // Exhaustive over every PluginHook variant — if a variant is added,
    // this match fails to compile until it's listed here.
    let cases = [
        (PluginHook::Import, "\"import\""),
        (PluginHook::Publish, "\"publish\""),
        (PluginHook::Deploy, "\"deploy\""),
        (PluginHook::Syndicate, "\"syndicate\""),
        (PluginHook::Process, "\"process\""),
    ];
    for (hook, expected) in cases {
        let json = serde_json::to_string(&hook).unwrap();
        assert_eq!(json, expected, "serialize {hook:?}");
        let parsed: PluginHook = serde_json::from_str(expected).unwrap();
        assert_eq!(parsed, hook, "deserialize {expected}");
    }
}

#[test]
fn trigger_context_serializes_all_variants_snake_case() {
    let cases = [
        (TriggerContext::OnboardingFlow, "\"onboarding_flow\""),
        (TriggerContext::SettingsManual, "\"settings_manual\""),
        (TriggerContext::Background, "\"background\""),
        (TriggerContext::ManualOne, "\"manual_one\""),
    ];
    for (ctx, expected) in cases {
        let json = serde_json::to_string(&ctx).unwrap();
        assert_eq!(json, expected, "serialize {ctx:?}");
        let parsed: TriggerContext = serde_json::from_str(expected).unwrap();
        assert_eq!(parsed, ctx, "deserialize {expected}");
    }
}

#[test]
fn plugin_manifest_without_capabilities_deserializes_to_empty() {
    // PluginManifest.capabilities is #[serde(default)] — a JSON manifest that
    // omits the "capabilities" key must deserialize successfully and produce an
    // empty Vec (no capabilities), rather than a serde error.
    let json = r#"{
            "name": "no-caps-plugin",
            "version": "0.1.0",
            "entry": "index.js"
        }"#;
    let manifest: PluginManifest =
        serde_json::from_str(json).expect("PluginManifest without 'capabilities' must parse");
    assert!(
        manifest.capabilities.is_empty(),
        "missing capabilities key must default to empty Vec, got {:?}",
        manifest.capabilities
    );
}

/// `ProjectInfo.folder_name` tells generator plugins what to name a folder home:
/// `None` makes a plugin write `index.md` instead of the self-named `<folder>.md`.
/// The old `folder_name_from_path` was a sixth restatement of `Path::file_name`, so
/// for a dot-path root it returned `None` and the plugin silently produced the wrong
/// filename. Taking the already-resolved `VaultRoot` removes the algorithm entirely.
#[test]
fn project_info_folder_name_survives_a_dot_path_root() {
    use crate::vault::paths::VaultRoot;

    let tmp = std::env::temp_dir().join(format!("moss_pi_dot_{}", uuid::Uuid::new_v4()));
    let site = tmp.join("潮汐");
    fs::create_dir_all(&site).unwrap();
    fs::write(site.join("index.md"), "---\ntitle: 首頁\n---\n內容\n").unwrap();

    let root = VaultRoot::resolve_in(std::path::Path::new("."), &site);
    let ps = crate::build::scan::scan::scan_folder(root.as_str()).expect("scan should succeed");
    assert_eq!(
        ProjectInfo::from_structure(&ps, &root)
            .folder_name
            .as_deref(),
        Some("潮汐"),
        "a `.` root must still tell plugins the folder name"
    );

    let _ = fs::remove_dir_all(&tmp);
}

// --- contributions fold forward into capabilities ---

#[test]
fn a_contributed_deploy_target_is_a_deploy_capability() {
    let json = r#"{
        "name": "ipfs",
        "version": "0.1.0",
        "entry": "main.bundle.js",
        "contributes": { "deploy_target": { "display_name": "IPFS" } }
    }"#;

    let manifest = PluginManifest::parse(json).expect("manifest must parse");

    assert!(
        manifest.has_capability(&Capability::Deploy),
        "a plugin that contributes a deploy target must reach the deploy menu \
         without also declaring the legacy capability"
    );
}

#[test]
fn a_contributed_channel_carries_login_and_import() {
    let json = r#"{
        "name": "matters",
        "version": "0.1.0",
        "entry": "main.bundle.js",
        "contributes": {
            "channel": {
                "display_name": "Matters",
                "imports": true,
                "login": true
            }
        }
    }"#;

    let manifest = PluginManifest::parse(json).expect("manifest must parse");

    assert!(manifest.has_capability(&Capability::Syndicate));
    assert!(manifest.has_capability(&Capability::Login));
    assert!(manifest.has_capability(&Capability::Import));
}

#[test]
fn a_channel_that_needs_no_account_gains_no_login() {
    let json = r#"{
        "name": "webhook",
        "version": "0.1.0",
        "entry": "main.bundle.js",
        "contributes": { "channel": {} }
    }"#;

    let manifest = PluginManifest::parse(json).expect("manifest must parse");

    assert!(manifest.has_capability(&Capability::Syndicate));
    assert!(
        !manifest.has_capability(&Capability::Login),
        "login is what the user has to do, not something every channel needs"
    );
    assert!(!manifest.has_capability(&Capability::Import));
}

#[test]
fn a_legacy_capability_list_still_works() {
    let json = r#"{
        "name": "github",
        "version": "0.1.0",
        "entry": "main.bundle.js",
        "capabilities": ["deploy"]
    }"#;

    let manifest = PluginManifest::parse(json).expect("manifest must parse");

    assert!(
        manifest.has_capability(&Capability::Deploy),
        "every installed plugin predates this capability fold; none may break on the \
         release that ships it"
    );
}

#[test]
fn declaring_both_vocabularies_does_not_duplicate() {
    let json = r#"{
        "name": "github",
        "version": "0.1.0",
        "entry": "main.bundle.js",
        "capabilities": ["deploy", "process"],
        "contributes": { "deploy_target": { "display_name": "GitHub Pages" } }
    }"#;

    let mut manifest = PluginManifest::parse(json).expect("manifest must parse");
    manifest.fold_contributions_into_capabilities();

    assert_eq!(
        manifest
            .capabilities
            .iter()
            .filter(|c| **c == Capability::Deploy)
            .count(),
        1,
        "folding is idempotent — a plugin mid-migration declares both"
    );
    assert!(manifest.has_capability(&Capability::Process));
}

/// The probe's verdict travels inside the one completion path every hook uses,
/// beside `deployment` — and the pre-contract `{ready}` shape a shipped hook
/// still answers with normalizes on the way in. (The shape grammar itself is
/// `plugins::setup`'s tests; this proves the ride-along.)
#[test]
fn a_verdict_rides_on_hook_result_in_either_wire_shape() {
    let result: HookResult = serde_json::from_str(r#"{"success":true,"setup":{"ready":true}}"#)
        .expect("a check_setup result must parse");
    assert!(result.setup.expect("the verdict rides on HookResult").is_ready());

    let result: HookResult =
        serde_json::from_str(r#"{"success":true,"setup":{"status":"ready"}}"#).expect("must parse");
    assert!(result.setup.expect("the contract shape rides the same").is_ready());
}

/// A deploy result from a plugin that has never heard of `check_setup` still
/// parses — the field is additive, and every shipped plugin predates it.
#[test]
fn a_result_without_a_verdict_still_parses() {
    let result: HookResult =
        serde_json::from_str(r#"{"success":true,"message":"deployed"}"#).expect("must parse");
    assert!(result.setup.is_none());
}
