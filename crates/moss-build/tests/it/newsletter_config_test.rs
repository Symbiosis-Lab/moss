//! `[channels.email] service` in `.moss/config.toml` reaches the host as
//! `subscribe.newsletter` of `_moss/commerce.json`.

use std::fs;
use std::path::Path;

use moss_build::build::{BuildTrigger, PipelineConfig, PluginMode};
use moss_build::moss_paths::MossPaths;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Builds a hosted site with a subscribe form and returns its `commerce.json`.
fn commerce(email_channel: &str) -> serde_json::Value {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".moss/config.toml", &format!("schema_version = 7\n\n[site]\nlang = \"zh-Hant\"\n\n{email_channel}"));
    write(root, ".moss/state.toml", "[deployment]\nsite_id = \"harbor-notes\"\ndeploy_method = \"moss\"\n");
    write(root, "index.md", "# 首頁\n");
    write(root, "subscribe.md", "# 訂閱\n\n:::subscribe\n- 電郵 * {type=email}\n- 稱呼 *\n:::\n");
    let turn = crate::site::turn();
    let result = tokio::runtime::Runtime::new().unwrap().block_on(crate::site::run(&turn, PipelineConfig {
        root: moss_build::vault_root::VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: moss_build::cli::host::cli_host_ports(root.to_str().unwrap()),
        trigger: BuildTrigger::Full,
        reason: None,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }));
    assert!(result.is_ok(), "build failed: {result:?}");
    let raw = fs::read_to_string(MossPaths::new(root).current_ptr().join("_moss/commerce.json")).expect("commerce.json is written");
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn the_service_is_published_beside_the_questions() {
    let v = commerce("[channels.email]\nservice = \"sendgrid\"\n");
    assert_eq!(v["subscribe"]["newsletter"], serde_json::json!({"provider": "sendgrid"}));
    assert!(v["subscribe"]["fields"][""].is_array(), "{v}");
}

#[test]
fn a_service_that_is_not_a_name_is_left_out_and_the_build_goes_on() {
    let v = commerce("[channels.email]\nservice = 3\n");
    assert!(v["subscribe"].get("newsletter").is_none(), "{v}");
    assert!(v["subscribe"]["fields"][""].is_array(), "the questions are still published: {v}");
}
