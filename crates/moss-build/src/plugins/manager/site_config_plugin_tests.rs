//! A fixture `process` plugin driven through the real engine: it reads
//! `site_config` from its hook context and writes binary and text files into
//! the project, so the context field, the base64 boundary and the sandbox are
//! exercised together.

use super::engine_adapter::{HookInvocation, HookSignalSink, PluginEngine, QuickJsEngineAdapter};
use super::hook_context::context_for_plugin;
use super::signal_route::HookSignal;
use crate::plugins::types::{ProcessContext, ProjectInfo};

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

    let plugins = crate::plugins::bundled::discover_plugins(pp).expect("discovery");
    let plugin = plugins.iter().find(|p| p.manifest.name == "site-config-writer").expect("fixture discovered");
    let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
    let ctx = context_for_plugin(pp, &ProcessContext::for_build(pp.to_string(), info), plugin);

    let bundle = std::fs::read_to_string(plugin_dir.join("main.bundle.js")).unwrap();
    let adapter = QuickJsEngineAdapter::new_for_test_multi_in(pp, vec![("site-config-writer", &bundle, "SiteConfigWriter")]);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink = HookSignalSink::new(move |s| { let _ = tx.send(s); });
    adapter
        .dispatch_hook(
            HookInvocation {
                plugin_name: "site-config-writer".into(),
                hook_name: "process".into(),
                context: serde_json::to_value(&ctx).unwrap(),
                project_path: pp.into(),
                moss_dir: format!("{pp}/.moss"),
            },
            sink,
        )
        .await
        .unwrap();
    let (success, error) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Some(HookSignal::Complete { success, error, .. }) => break (success, error),
                Some(_) => continue,
                None => panic!("sink closed before Complete"),
            }
        }
    })
    .await
    .expect("hook completes");
    adapter.teardown().await;

    assert!(success, "hook failed: {error:?}");
    assert_eq!(std::fs::read(project.join("maps/sheet.bin")).unwrap(), vec![3u8, 0xff, 0x00]);
    assert_eq!(std::fs::read_to_string(project.join("maps/credential-visible.txt")).unwrap(), "undefined");
    assert_eq!(std::fs::read_to_string(project.join("maps/fence.txt")).unwrap(), "refused");
    assert!(!project.join(".moss/stolen.bin").exists());
}
