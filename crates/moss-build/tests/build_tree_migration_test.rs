//! A build on a folder still laid out the old way — one `.moss/build` root
//! holding the store, plus a root a cloud sync client renamed aside — comes
//! out with the split tree: the store at `.moss/cache`, both roots gone, and
//! every blob they held. The migration itself is unit-tested beside its
//! code; this proves the pipeline runs it, first, before anything writes.

use std::path::Path;

use moss_build::build::cache::ObjectStore;

fn blob(dir: &Path, bytes: &[u8]) -> String {
    ObjectStore::new(dir.to_path_buf()).store_bytes(bytes).expect("stored")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_build_moves_an_old_tree_into_the_split_layout_before_writing() {
    use moss_build::build::{null_sink, run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new().prefix("moss-tree-migration").tempdir_in(&base).unwrap();
    let folder = tmp.path().canonicalize().unwrap();
    std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    let moss = folder.join(".moss");
    let legacy = moss.join("build");
    let aside = moss.join("build 2");
    let in_legacy = blob(&legacy.join("cache").join("objects"), b"encoded on this machine");
    let in_aside = blob(&aside.join("cache").join("objects"), b"encoded before the swap");

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

    let store = ObjectStore::new(moss.join("cache").join("objects"));
    assert!(store.blob_path(&in_legacy).exists(), "the old root's blob is in the store");
    assert!(store.blob_path(&in_aside).exists(), "and so is the renamed-aside root's");
    assert!(!legacy.exists(), "the old root is gone");
    assert!(!aside.exists(), "and so is the sibling");
    assert!(moss.join("build.nosync").join("staging").join("index.html").exists(), "the build wrote the split tree");
}
