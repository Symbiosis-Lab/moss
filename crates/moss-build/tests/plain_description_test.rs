//! An automatic description is plain text: no heading mark, emphasis mark or
//! link bracket from the Markdown it was read from, in every tag that carries it.

use std::fs;
use std::path::{Path, PathBuf};

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

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn home_html(home_body: &str) -> String {
    let tmp = std::env::temp_dir().join(format!("moss_plain_desc_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", &format!("---\ntitle: Example Garden\n---\n\n{home_body}\n"));
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    fs::read_to_string(tmp.join(".moss/build.nosync/staging/index.html")).unwrap()
}

fn assert_description(html: &str, want: &str) {
    for needle in [
        format!("<meta name=\"description\" content=\"{want}\""),
        format!("<meta property=\"og:description\" content=\"{want}\""),
        format!("<meta name=\"twitter:description\" content=\"{want}\""),
        format!("\"description\":\"{want}\""),
    ] {
        assert!(html.contains(&needle) || html.contains(&needle.replace("\":\"", "\": \"")), "missing {needle}\n{html}");
    }
}

#[test]
fn a_heading_inside_the_opening_picture_block_is_described_without_its_mark() {
    let html = home_html(":::hero {image=chart.jpg}\n# A sentence about the work.\n:::");
    assert_description(&html, "A sentence about the work.");
}

#[test]
fn a_list_inside_the_opening_picture_block_is_not_published_as_source() {
    let html = home_html(":::hero {image=chart.jpg}\n- a point\n:::\n\nPlants grow slowly.");
    assert!(!html.contains("content=\"- a point"), "{html}");
    assert_description(&html, "Plants grow slowly.");
}
