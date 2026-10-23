//! A CommonMark image title reaches the built `<img>` as its `title`
//! attribute, in a captioned figure and in running text alike. Built through
//! the real pipeline, because the snapshot-aware image path is where the title
//! used to be dropped.

use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn build_sync(folder_path: &str) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}


// A valid 4x4 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x02, 0x00, 0x00, 0x00, 0x26, 0x93, 0x09,
    0x29, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x47, 0x0c, 0xc4, 0x71, 0x00, 0xae, 0x93, 0x0f, 0xf1, 0xd0, 0x5f, 0x23, 0x9e, 0x00, 0x00, 0x00,
    0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn an_image_title_becomes_the_img_title_attribute() {
    let root = std::env::temp_dir().join(format!("moss_image_title_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    write(&root, ".moss/config.toml", "[site]\ndomain = \"example.com\"\n");
    fs::create_dir_all(root.join("photos")).unwrap();
    fs::write(root.join("photos/a.png"), PNG).unwrap();
    write(
        &root,
        "index.md",
        "---\ntitle: Home\n---\n\n![Caption words](photos/a.png \"Hover \\\"words\\\" style=x\")\n\nText ![Inline alt](photos/a.png \"Inline title\") here.\n\n![No title](photos/a.png)\n\n:::gallery\n![G alt](photos/a.png \"Gallery title\")\n:::\n",
    );
    build_sync(&root.to_string_lossy()).expect("build failed");
    let html = fs::read_to_string(root.join(".moss/build.nosync/staging/index.html")).unwrap();

    assert!(html.contains(r#"title="Hover &quot;words&quot; style=x""#), "figure image keeps its title: {html}");
    assert!(html.contains("<figcaption>Caption words</figcaption>"), "the bracket text stays the caption");
    assert!(html.contains(r#"title="Inline title""#) && html.contains(r#"alt="Inline alt""#), "inline image: {html}");

    assert!(html.contains(r#"title="Gallery title""#), "gallery image keeps its title: {html}");
    assert!(html.contains("background-image"), "the placeholder style is still emitted beside a title");
}
