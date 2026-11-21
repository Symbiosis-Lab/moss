//! Under `moss build --serve --watch`, a picture added after the server started
//! is shown as its original until its encode lands.
//!
//! The server is handed the registry of the build that started it, and every
//! watch rebuild runs with a new set of host ports from the same factory. If a
//! rebuild promised into a registry of its own, the server could not find the
//! original for the new picture's variant URLs and answered 404 until the
//! encode wrote them. This replays the headless driver's order by hand (the
//! folder session is registered before the host ports are built), drives two
//! builds and asks the server's handle.

use std::path::Path;

async fn build(turn: &crate::site::Turn, folder: &Path) {
    use moss_build::build::{null_sink, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    crate::site::run(turn, PipelineConfig {
        root: VaultRoot::resolve(folder),
        progress: null_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder.to_string_lossy()),
        trigger: BuildTrigger::Full, reason: None,
        exits_after_build: true,
        site_url_override: Some("https://example.com".to_string()),
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    })
    .await
    .expect("build");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_picture_added_under_watch_is_served_as_its_original() {
    let turn = crate::site::turn();
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new().prefix("moss-watch-stand-in").tempdir_in(&base).unwrap();
    let folder = tmp.path().canonicalize().unwrap();
    let folder_str = folder.to_string_lossy().to_string();
    std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();

    // The headless driver registers the folder's session, then takes the
    // registry it gives the server from the initial build's host ports.
    let _session = moss_build::system::folder_session::register_session(&folder_str);
    let server_registry = moss_build::cli::host::cli_host_ports(&folder_str)
        .services
        .assets
        .expect("a headless build has a registry");
    build(&turn, &folder).await;

    let picture = folder.join("photo.jpg");
    image::RgbImage::from_fn(1600, 1000, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]))
        .save(&picture)
        .unwrap();
    std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\n![A photo](photo.jpg)\n").unwrap();
    build(&turn, &folder).await;

    for url in ["photo.webp", "photo.w800.webp", "photo.w1200.webp"] {
        assert_eq!(
            server_registry.source_passthrough(url).as_deref(),
            Some(picture.as_path()),
            "the server must find the original for {url}"
        );
    }
}
