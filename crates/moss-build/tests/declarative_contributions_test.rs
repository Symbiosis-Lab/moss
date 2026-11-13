//! Plugin blocks: what the build makes of a `:::<short>` fence when the plugin
//! is there, when it is not, and when two plugins want the same name.
use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use serde_json::json;
use std::path::Path;
use std::sync::LazyLock;

/// Build locks live in the per-user home, so a build here must not reach the real one.
static HOME: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("MOSS_HOME", home.path());
    home
});

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn build(root: &Path, mode: PluginMode) {
    build_for(root, mode, BuildTrigger::Full)
}

fn build_for(root: &Path, mode: PluginMode, trigger: BuildTrigger) {
    LazyLock::force(&HOME);
    let result = tokio::runtime::Runtime::new().unwrap().block_on(run_pipeline(PipelineConfig {
        root: moss_build::vault_root::VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins: mode,
        watch: false,
        start_server: false,
        host: moss_build::cli::host::cli_host_ports(root.to_str().unwrap()),
        trigger,
        exits_after_build: true,
        site_url_override: Some("https://example.com".into()),
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: vec![],
    }));
    assert!(result.is_ok(), "{result:?}");
}

fn card() -> serde_json::Value {
    json!({"blocks":{"card":{"element":"demo-card","script":"view.js","stylesheet":"view.css",
        "attributes":[{"key":"label"},{"key":"size","type":"number"}]}}})
}

fn plugin(root: &Path, name: &str, contributes: serde_json::Value) {
    let manifest = json!({"name":name, "version":"1.0.0", "entry":"main.js", "contributes":contributes});
    write(root, &format!(".moss/plugins/{name}/manifest.json"), &manifest.to_string());
    write(root, &format!(".moss/plugins/{name}/main.js"), "throw new Error('declarations must not execute');");
    write(root, &format!(".moss/plugins/{name}/view.js"), "customElements.define('demo-card', class extends HTMLElement {});");
    write(root, &format!(".moss/plugins/{name}/view.css"), "demo-card { display: block; }");
}

fn output(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(".moss/build.nosync/current").join(path)).unwrap()
}

fn site() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), ".moss/config.toml", "schema_version = 6\n");
    write(tmp.path(), "plain.md", "# Plain\n\nOrdinary text.\n");
    tmp
}

const PAGE: &str = "# Home\n\n:::card {label=\"a\\\"&<b\" size=big extra=bad}\n**Fallback**\n:::\n\n:::catalog--card\nSecond fallback.\n:::\n";

#[test]
fn a_block_wraps_its_body_escapes_attributes_and_loads_assets_once_on_using_pages() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", PAGE);
    plugin(root, "catalog", card());
    build(root, PluginMode::SlotsOnly);
    let html = output(root, "index.html");
    assert_eq!(html.matches("<demo-card").count(), 2, "{html}");
    assert!(html.contains("<strong>Fallback</strong>"));
    assert!(html.contains(r#"label="a&quot;&amp;&lt;b""#), "{html}");
    assert!(!html.contains("extra="), "an undeclared attribute is dropped");
    assert!(!html.contains("size="), "an invalid value is dropped");
    assert!(!html.contains(r#"label="Default""#), "a manifest default is never emitted");
    assert_eq!(html.matches("/_moss/plugins/catalog/view.js").count(), 1, "{html}");
    assert_eq!(html.matches("/_moss/plugins/catalog/view.css").count(), 1, "{html}");
    assert!(!output(root, "plain/index.html").contains("/_moss/plugins/"));
    assert!(output(root, "_moss/plugins/catalog/view.js").contains("customElements.define"));
}

#[test]
fn a_block_body_gets_every_pass_ordinary_markdown_gets() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", "# Home\n\n::::card\n[Plain](plain.md)\n\n[[plain]]\n\n:::buttons\n[Go](plain.md)\n:::\n::::\n");
    write(root, "grid.md", "# Grid\n\n::::grid 2\n:::card\nIn a cell.\n:::\n+++\nOther.\n::::\n");
    write(root, "region.md", "# Region\n\n::::{.band}\n:::card\nIn a region.\n:::\n::::\n");
    plugin(root, "catalog", card());
    build(root, PluginMode::SlotsOnly);
    let html = output(root, "index.html");
    assert!(html.contains(r#"href="/plain/""#), "a body link resolves: {html}");
    assert!(html.contains("moss-buttons") || html.contains("<a class"), "a nested shortcode renders: {html}");
    assert!(!html.contains("plain.md"), "{html}");
    for page in ["grid/index.html", "region/index.html"] {
        let html = output(root, page);
        assert!(html.contains("<demo-card"), "{page}: {html}");
        assert_eq!(html.matches("/_moss/plugins/catalog/view.js").count(), 1, "{page}: {html}");
    }
}

#[test]
fn the_body_stands_alone_without_the_plugin_or_with_it_disabled() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", PAGE);
    plugin(root, "catalog", card());
    let bare = |html: &str| {
        assert!(!html.contains("<demo-card"), "{html}");
        assert!(!html.contains("/_moss/plugins/"), "{html}");
        assert!(html.contains("<strong>Fallback</strong>"), "{html}");
        assert!(html.contains("Second fallback."), "{html}");
    };
    build(root, PluginMode::Skip);
    bare(&output(root, "index.html"));
    write(root, ".moss/plugins/catalog/config.toml", "enabled = false\n");
    build(root, PluginMode::SlotsOnly);
    bare(&output(root, "index.html"));
}

#[test]
fn a_shared_short_name_places_neither_but_the_full_name_still_works() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", PAGE);
    plugin(root, "catalog", card());
    plugin(root, "another", card());
    build(root, PluginMode::SlotsOnly);
    let html = output(root, "index.html");
    assert_eq!(html.matches("<demo-card").count(), 1, "only the full name resolves: {html}");
    assert!(html.contains("<strong>Fallback</strong>"), "the short fence still shows its body");
}

#[test]
fn changing_a_block_asset_rerenders_an_unchanged_page() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", PAGE);
    plugin(root, "catalog", card());
    build(root, PluginMode::SlotsOnly);
    let address = |html: &str| html.split("view.js?v=").nth(1).unwrap().split('"').next().unwrap().to_string();
    let first = address(&output(root, "index.html"));
    write(root, ".moss/plugins/catalog/view.js", "customElements.define('demo-card', class extends HTMLElement { connectedCallback() {} });");
    build(root, PluginMode::SlotsOnly);
    let second = output(root, "index.html");
    assert_ne!(first, address(&second), "the page must load the new script: {second}");
    assert!(output(root, "_moss/plugins/catalog/view.js").contains("connectedCallback"));
    build(root, PluginMode::SlotsOnly);
    assert_eq!(address(&output(root, "index.html")), address(&second), "an unchanged rebuild keeps the tag");
}

#[test]
fn a_block_asset_cannot_be_a_symlink_out_of_the_plugin() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", PAGE);
    write(root, "outside.js", "secret");
    plugin(root, "catalog", card());
    let linked = root.join(".moss/plugins/catalog/view.js");
    std::fs::remove_file(&linked).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("outside.js"), &linked).unwrap();
    #[cfg(not(unix))]
    return;
    build(root, PluginMode::SlotsOnly);
    let html = output(root, "index.html");
    assert!(!html.contains("view.js"), "{html}");
    assert!(html.contains("view.css"), "the other asset still loads: {html}");
    assert!(!root.join(".moss/build.nosync/current/_moss/plugins/catalog/view.js").exists());
}

#[test]
fn a_page_an_incremental_build_could_carry_loads_the_changed_block_asset() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", PAGE);
    plugin(root, "catalog", card());
    build(root, PluginMode::SlotsOnly);
    let address = |html: &str| html.split("view.js?v=").nth(1).unwrap().split('"').next().unwrap().to_string();
    let edit = |later: &str| {
        write(root, "plain.md", &format!("# Plain\n\nOrdinary text.\n\n{later}\n"));
        build_for(root, PluginMode::SlotsOnly, BuildTrigger::ContentOnly(vec![root.join("plain.md")]));
    };
    edit("A warm cache needs one incremental build behind it.");
    let first = address(&output(root, "index.html"));
    write(root, ".moss/plugins/catalog/view.js", "customElements.define('demo-card', class extends HTMLElement { connectedCallback() {} });");
    edit("An edited later paragraph.");
    assert_ne!(first, address(&output(root, "index.html")), "the untouched page must not keep the old address");
}

#[test]
fn a_block_in_a_footer_loads_its_assets_on_every_page_once() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", "# Home\n\n:::card\nOn the page too.\n:::\n");
    write(root, "footer.md", ":::card\nIn the footer.\n:::\n");
    plugin(root, "catalog", card());
    build(root, PluginMode::SlotsOnly);
    for page in ["index.html", "plain/index.html"] {
        let html = output(root, page);
        assert!(html.contains("In the footer."), "{page}: {html}");
        assert_eq!(html.matches("/_moss/plugins/catalog/view.js").count(), 1, "{page}: {html}");
    }
}

#[test]
fn public_data_is_published_under_the_plugin_and_never_without_it() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", "# Home\n\nWelcome.\n");
    write(root, ".moss/config.toml", "schema_version = 6\n[services.forms]\nendpoint = 'https://example.com/forms'\n[services.catalog]\nendpoint = 'https://example.com/catalog'\nprivate_token = 'unpublished'\n");
    plugin(root, "catalog", json!({"public_data":{"keys":["endpoint"]}}));
    build(root, PluginMode::SlotsOnly);
    let data: serde_json::Value = serde_json::from_str(&output(root, "_moss/extensions/catalog.json")).unwrap();
    assert_eq!(data, json!({"schema":1,"data":{"endpoint":"https://example.com/catalog"}}));
    assert!(output(root, "_moss/commerce.json").contains("forms"), "core forms data is untouched");
    assert!(!output(root, "_moss/commerce.json").contains("catalog"), "a plugin's data stays out of the core file");
    let published = root.join(".moss/build.nosync/current/_moss/extensions/catalog.json");
    build(root, PluginMode::Skip);
    assert!(!published.exists(), "plugins off publishes nothing");
    write(root, ".moss/plugins/catalog/config.toml", "enabled = false\n");
    build(root, PluginMode::SlotsOnly);
    assert!(!published.exists(), "a disabled plugin publishes nothing");
}

#[test]
fn a_plugin_with_no_code_places_a_block_and_publishes_data() {
    let tmp = site();
    let root = tmp.path();
    write(root, "index.md", "# Home\n\n:::card\nFallback.\n:::\n");
    write(root, ".moss/config.toml", "schema_version = 6\n[services.catalog]\nendpoint = 'https://example.com/catalog'\n");
    let manifest = json!({"name":"catalog","version":"1.0.0","contributes":{
        "blocks":{"card":{"element":"demo-card"}},
        "public_data":{"keys":["endpoint"]}}});
    write(root, ".moss/plugins/catalog/manifest.json", &manifest.to_string());
    build(root, PluginMode::SlotsOnly);
    assert!(output(root, "index.html").contains("<demo-card>"));
    assert!(output(root, "_moss/extensions/catalog.json").contains("example.com/catalog"));
}

#[test]
fn an_incremental_build_publishes_edited_settings_and_a_changed_allowlist() {
    let tmp = site();
    let root = tmp.path();
    let config = |endpoint: &str| format!("schema_version = 6\n[services.catalog]\nendpoint = '{endpoint}'\nregion = 'north'\n");
    let declare = |keys: serde_json::Value| plugin(root, "catalog", json!({"public_data":{"keys":keys}}));
    write(root, ".moss/config.toml", &config("https://example.com/one"));
    write(root, "index.md", "# Home\n");
    declare(json!(["endpoint"]));
    build(root, PluginMode::SlotsOnly);
    let edit = |later: &str| {
        write(root, "plain.md", &format!("# Plain\n\n{later}\n"));
        build_for(root, PluginMode::SlotsOnly, BuildTrigger::ContentOnly(vec![root.join("plain.md")]));
    };
    edit("A warm cache.");
    write(root, ".moss/config.toml", &config("https://example.com/two"));
    edit("After a settings edit.");
    assert!(output(root, "_moss/extensions/catalog.json").contains("example.com/two"));
    declare(json!(["endpoint", "region"]));
    edit("After an allowlist change.");
    assert!(output(root, "_moss/extensions/catalog.json").contains("north"));
    write(root, ".moss/plugins/catalog/config.toml", "enabled = false\n");
    edit("After disabling the plugin.");
    assert!(!root.join(".moss/build.nosync/current/_moss/extensions/catalog.json").exists(), "a disabled plugin's file does not linger");
}
