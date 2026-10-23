//! A fence attribute block that does not parse used to vanish without a
//! trace: `:::hero {image=photos/x.png|cover bottom}` built clean and the
//! hero lost its image. It must now count as a build problem (which
//! `--strict` fails on); the quoted form must stay silent.

use std::fs;
use std::path::{Path, PathBuf};

use moss_build::build::cli_output::take_cli_problems;

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

fn write(root: &Path, rel: &str, body: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// Build a one-page site whose hero carries `attrs`; return the build's problem count.
fn problems_for(attrs: &str) -> usize {
    let tmp = std::env::temp_dir().join(format!("moss_attr_warn_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", b"[site]\ndomain = \"example.com\"\n");
    write(&tmp, "photos/harbor.png", PNG);
    write(
        &tmp,
        "index.md",
        format!("---\ntitle: Home\n---\n\n:::hero {attrs}\n# Harbor\n:::\n").as_bytes(),
    );
    let _ = take_cli_problems();
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    take_cli_problems()
}

#[test]
fn unparseable_hero_attributes_are_a_build_problem_and_quoted_ones_are_not() {
    let quoted = problems_for("{image=\"photos/harbor.png|cover bottom\"}");
    let unquoted = problems_for("{image=photos/harbor.png|cover bottom}");
    let unquoted_short = problems_for("{image=photos/harbor.png|cover}");
    assert_eq!(quoted, 0, "the quoted form must build without a problem");
    assert!(unquoted >= 1, "unquoted `|cover bottom` must warn, got {unquoted}");
    assert!(unquoted_short >= 1, "unquoted `|cover` must warn, got {unquoted_short}");
}
