//! A build on a folder still laid out the oldest way — one `.moss/build` root,
//! plus a root a cloud sync client renamed aside — writes its output to this
//! computer's store and removes both roots from the folder. The migration
//! itself is unit-tested beside its code; this proves the pipeline runs it.

use std::path::Path;

use moss_build::build::cache::ObjectStore;

fn blob(dir: &Path, bytes: &[u8]) -> String {
    ObjectStore::new(dir.to_path_buf()).store_bytes(bytes).expect("stored")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_build_removes_the_oldest_output_roots_from_the_folder() {
    let turn = crate::site::turn();
    use moss_build::build::{null_sink, BuildTrigger, PipelineConfig, PluginMode};
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
    blob(&legacy.join("cache").join("objects"), b"encoded on this machine");
    blob(&aside.join("cache").join("objects"), b"encoded before the swap");

    let folder_str = folder.to_string_lossy().to_string();
    crate::site::run(&turn, PipelineConfig {
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
        rename_pairs: Vec::new(),
    })
    .await
    .expect("build");

    let paths = moss_build::moss_paths::MossPaths::new(&folder);
    assert!(paths.staging_dir().join("index.html").exists(), "the build wrote its output to the store");
    assert!(!paths.staging_dir().starts_with(&folder));
    // Removed in the background, a step per build; these hold no generations.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while (legacy.exists() || aside.exists()) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!legacy.exists(), "the old root is gone");
    assert!(!aside.exists(), "and so is the sibling");
    assert!(!moss.join("build.nosync").exists(), "and nothing is written in their place");
}
