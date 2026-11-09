//! A fixture `process` plugin driven through the real manager and engine as
//! the person's run: it reads `site_config` from its hook context and writes
//! binary and text files into the project, so the context field, the base64
//! boundary and the sandbox are exercised together.

use super::PluginManager;
use crate::plugins::adapter_host::AdapterHost;
use crate::plugins::types::{ProcessContext, ProjectInfo, TriggerContext};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plugins/site-config-writer");

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_plugin_reads_site_config_and_writes_binary_files() {
    let target = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp");
    std::fs::create_dir_all(target).unwrap();
    let container = tempfile::TempDir::new_in(target).unwrap();
    let project = container.path().join("project");
    let plugin_dir = project.join(".moss/plugins/site-config-writer");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    for file in ["manifest.json", "main.bundle.js"] {
        std::fs::copy(format!("{FIXTURE}/{file}"), plugin_dir.join(file)).unwrap();
    }
    std::fs::write(
        project.join(".moss/config.toml"),
        "[map.layers.sheet]\nzoom = 3\n[services.email]\napi_key = \"sk-live\"\n",
    )
    .unwrap();
    let pp = project.to_str().unwrap();

    let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
    let manager = PluginManager::new(pp, AdapterHost::headless(crate::build::ports::reporter::discard_owned())).unwrap();
    manager
        .execute_process_for_plugin("site-config-writer", &ProcessContext::new(pp.into(), info, TriggerContext::SettingsManual))
        .await
        .expect("the person's run writes the folder");

    assert_eq!(std::fs::read(project.join("maps/sheet.bin")).unwrap(), vec![3u8, 0xff, 0x00]);
    assert_eq!(std::fs::read_to_string(project.join("maps/credential-visible.txt")).unwrap(), "undefined");
    assert_eq!(std::fs::read_to_string(project.join("maps/fence.txt")).unwrap(), "refused");
    assert!(!project.join(".moss/stolen.bin").exists());
}
