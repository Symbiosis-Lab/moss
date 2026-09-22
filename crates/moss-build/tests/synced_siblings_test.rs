//! A build on a folder where a cloud sync client left a `state 2.toml` beside
//! `state.toml` comes out with one file holding what both knew. The merge is
//! unit-tested beside its code; this proves the pipeline runs it at start.

use std::path::Path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_build_folds_a_conflict_sibling_into_the_state_file() {
    use moss_build::build::{null_sink, run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new().prefix("moss-synced-siblings").tempdir_in(&base).unwrap();
    let folder = tmp.path().canonicalize().unwrap();
    std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    let moss = folder.join(".moss");
    std::fs::create_dir_all(&moss).unwrap();
    std::fs::write(moss.join("state.toml"), "[deployment]\nmethod = \"moss\"\n").unwrap();
    std::fs::write(moss.join("state 2.toml"), "[deployment]\nmethod = \"onion\"\ndomain = \"x.onion\"\n").unwrap();

    let folder_str = folder.to_string_lossy().to_string();
    run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&folder),
        progress: null_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder_str),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: Some("https://example.com".to_string()),
        server_port: None,
        admission_epoch: None,
        live_port: None,
    })
    .await
    .expect("build");

    let state = std::fs::read_to_string(moss.join("state.toml")).unwrap();
    assert!(state.contains("method = \"moss\""), "the canonical value wins:\n{state}");
    assert!(state.contains("domain = \"x.onion\""), "the sibling's key was taken:\n{state}");
    assert!(!moss.join("state 2.toml").exists(), "the sibling is gone");
}
