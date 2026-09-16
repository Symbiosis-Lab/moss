use moss_core::contract::components::COMPONENTS;
use moss_core::contract::describe::{
    CliCommandInfo, DescribePayload, ManifestFieldInfo, PluginHookInfo, SlotInfo,
    DESCRIBE_SCHEMA_VERSION, MOSS_HTML_VERSION,
};
use moss_core::contract::tokens::{load_tokens, parse_tokens};

#[test]
fn describe_payload_serializes_with_versions() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");

    assert_eq!(json["describe_schema_version"], DESCRIBE_SCHEMA_VERSION);
    assert_eq!(json["moss_html_version"], MOSS_HTML_VERSION);
    assert!(json["tokens"].is_object());
    assert!(json["components"].is_array());
    // Internal classes (moss-apply*) are filtered out of the public contract.
    // The payload length must be COMPONENTS.len() minus the internal class count.
    let public_count = COMPONENTS.iter().filter(|c| c.is_public()).count();
    assert_eq!(
        json["components"].as_array().unwrap().len(),
        public_count,
        "payload must contain exactly the public (non-internal) components"
    );
    // Internal classes must not appear in the payload.
    let components = json["components"].as_array().unwrap();
    assert!(
        components.iter().all(|c| !c["class"].as_str().unwrap_or("").starts_with("moss-apply")),
        "internal moss-apply* classes must not appear in the public payload"
    );
}

#[test]
fn describe_schema_version_is_6() {
    // Guard against accidental version rollback. Any future bump must be
    // intentional and accompanied by a CHANGELOG entry.
    // v6 added `custom_properties` and `scope_attributes` — the escape-hatch
    // theming API and the structural attributes on classless elements.
    assert_eq!(DESCRIBE_SCHEMA_VERSION, 6);
}

#[test]
fn describe_payload_includes_token_groups() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");

    let tokens_obj = json["tokens"].as_object().expect("tokens object");
    assert!(tokens_obj.contains_key("typography"));
    assert!(tokens_obj.contains_key("color"));
}

#[test]
fn describe_payload_authorable_flag_is_set_correctly() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");

    let components = json["components"].as_array().expect("components array");

    // At least one component must have authorable == true.
    assert!(
        components.iter().any(|c| c["authorable"] == true),
        "at least one component must have authorable == true"
    );

    // moss-grid must be authorable (it is a ShortcodeKind::Grid root class).
    let moss_grid = components
        .iter()
        .find(|c| c["class"] == "moss-grid")
        .expect("moss-grid must be in payload");
    assert_eq!(
        moss_grid["authorable"], true,
        "moss-grid must have authorable == true"
    );

    // moss-card is NOT authorable (it is not a shortcode root class).
    let moss_card = components
        .iter()
        .find(|c| c["class"] == "moss-card")
        .expect("moss-card must be in payload");
    assert_eq!(
        moss_card["authorable"], false,
        "moss-card must have authorable == false"
    );
}

#[test]
fn describe_token_json_includes_dark_value() {
    let json = r##"{
        "$order": ["color"],
        "color": { "moss-color-bg": {"$type":"color",
            "$value":{"light":"#faf8f5","dark":"#1c1914"}} }
    }"##;
    let tokens = parse_tokens(json).unwrap();
    let payload = DescribePayload::new(&tokens);
    let bg = payload.tokens.get("color").unwrap().iter()
        .find(|t| t.name == "moss-color-bg").unwrap();
    assert_eq!(bg.dark_value, Some("#1c1914"));

    // Light-only token must omit dark_value entirely at the JSON layer
    // (skip_serializing_if = "Option::is_none" must produce no key, not null).
    let light_only = r##"{"$order":["color"],"color":{"moss-color-accent":{"$type":"color","$value":"#2d5a2d"}}}"##;
    let tokens2 = parse_tokens(light_only).unwrap();
    let payload2 = DescribePayload::new(&tokens2);
    let v = serde_json::to_value(&payload2).unwrap();
    assert!(
        v["tokens"]["color"][0].get("dark_value").is_none(),
        "light-only token must have no dark_value key in JSON output (not null)"
    );
}

/// `DescribePayload::new()` in moss-core produces empty plugin-contract vecs
/// (those are filled by the Tauri layer which has access to the plugin types).
/// The serialized JSON must include the keys with empty arrays — not absent keys.
#[test]
fn describe_payload_plugin_contract_keys_present_as_empty_arrays() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");

    assert!(json["plugin_hooks"].is_array(), "plugin_hooks must serialize as array");
    assert!(json["manifest_fields"].is_array(), "manifest_fields must serialize as array");
    assert!(json["slots"].is_array(), "slots must serialize as array");
    assert!(json["cli_commands"].is_array(), "cli_commands must serialize as array");
}

/// The plugin contract structs must round-trip through serde correctly.
/// This tests that field names, types, and required flags serialize as expected.
#[test]
fn describe_plugin_contract_structs_serialize_correctly() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens).with_plugin_contract(
        vec![PluginHookInfo {
            name: "process",
            description: "Pre-process files before generation.",
            arity: "multiple",
            context: "ProcessContext",
        }],
        vec![ManifestFieldInfo {
            name: "name",
            r#type: "string",
            required: true,
            description: "Plugin identifier.",
        }],
        vec![SlotInfo {
            name: "head-end",
            position: "Before </head>.",
            authorable: false,
        }],
        vec![CliCommandInfo {
            name: "build",
            args: "<folder> [--serve]",
            description: "Build the site.",
        }],
    );

    let json = serde_json::to_value(&payload).expect("serialize");

    // plugin_hooks
    let hooks = json["plugin_hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["name"], "process");
    assert_eq!(hooks[0]["arity"], "multiple");
    assert_eq!(hooks[0]["context"], "ProcessContext");

    // manifest_fields
    let fields = json["manifest_fields"].as_array().unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0]["name"], "name");
    assert_eq!(fields[0]["type"], "string");
    assert_eq!(fields[0]["required"], true);

    // slots
    let slots = json["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0]["name"], "head-end");
    assert_eq!(slots[0]["authorable"], false);

    // cli_commands
    let cmds = json["cli_commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0]["name"], "build");
    assert!(cmds[0]["args"].as_str().unwrap().contains("--serve"));
}

/// The seven hooks the 2026-08-03 audit found invisible.
///
/// okagaki and 在場 — the two most heavily customized moss sites — between them
/// set six custom properties and hung their layouts off three structural
/// attributes. Before schema v6, `describe --json` reported **none** of them,
/// so an agent given moss's own contract could not have written either theme.
/// This is the acceptance test for that gap.
#[test]
fn the_hooks_the_flagship_themes_actually_used_are_discoverable() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");
    let blob = json.to_string();

    for hook in [
        // set by okagaki
        "--moss-hero-object-position",
        // set by 在場
        "--moss-grid-image-ratio",
        "--moss-grid-image-radius",
        "--moss-grid-image-fit",
        "--moss-card-cover-ratio",
        "--moss-card-cover-fit",
        // the cap okagaki hand-fought across three selectors
        "--moss-hero-max-height",
        // structural attributes both themes scoped to
        "data-page",
        "data-mobile",
        "data-columns",
    ] {
        assert!(
            blob.contains(hook),
            "{hook} must be discoverable from `moss describe --json` — it was \
             load-bearing in a shipped moss theme and reported nowhere"
        );
    }
}

/// Every custom property must carry the fallback moss's CSS actually uses.
/// A declared hook with an invented default is worse than an undeclared one:
/// an agent reasons about the wrong starting point and cannot tell.
#[test]
fn custom_properties_declare_a_default_and_an_owner() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");

    let props = json["custom_properties"].as_array().expect("custom_properties array");
    assert!(props.len() >= 15, "expected the full escape-hatch set, got {}", props.len());
    for p in props {
        let name = p["name"].as_str().unwrap_or_default();
        assert!(name.starts_with("--moss-"), "{name} must be a --moss-* property");
        for field in ["owner", "default", "description"] {
            assert!(
                !p[field].as_str().unwrap_or_default().is_empty(),
                "{name} must declare a non-empty {field}"
            );
        }
    }
}

/// The shipped skill tells an agent to get the language allowlist from
/// `moss describe --json`. That instruction is only safe while the field is
/// actually there, so pin it.
///
/// The failure this guards is silent in both directions: an unrecognized
/// language directory is not a build error (the tree becomes ordinary
/// content), and a doc pointing at a field that does not exist costs the
/// agent a turn and teaches it to distrust the rest of the file. moss's own
/// styling reference shipped three dead paths for exactly that reason.
#[test]
fn describe_publishes_the_language_allowlist() {
    let tokens = load_tokens().expect("tokens");
    let payload = DescribePayload::new(&tokens);
    let json = serde_json::to_value(&payload).expect("serialize");

    let langs = json["languages"]
        .as_array()
        .expect("describe --json must carry a `languages` array");

    // The two the nav switcher can actually resolve, plus a long-tail code
    // and a region variant — enough to prove this is the real allowlist and
    // not a placeholder.
    for expected in ["en", "zh-hant", "zh-hans", "ja", "en-gb"] {
        assert!(
            langs.iter().any(|l| l.as_str() == Some(expected)),
            "`{expected}` must appear in describe --json languages, got {langs:?}"
        );
    }

    assert!(
        langs.len() >= 40,
        "expected the full allowlist (~51 codes), got {}",
        langs.len()
    );

    // `english` is the name an agent invents when it cannot read the list.
    assert!(
        !langs.iter().any(|l| l.as_str() == Some("english")),
        "the allowlist must not contain invented long names"
    );
}
