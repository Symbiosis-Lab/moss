//! A text embed no plugin can fill is a build problem, except under
//! `--no-plugins`, where plugins were skipped and the missing text is not the
//! author's fault. One test, because the problem counter is process-wide.

use std::fs;

use moss_build::build::cli_output::take_cli_problems;
use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use moss_build::cli::host::cli_host_ports;
use moss_build::vault_root::VaultRoot;

fn build_once(files: &[(&str, &str)], plugins: PluginMode) -> usize {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        fs::create_dir_all(dir.path().join(name).parent().unwrap()).unwrap();
        fs::write(dir.path().join(name), body).unwrap();
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let folder = dir.path().to_string_lossy().to_string();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(dir.path()),
        progress: moss_build::build::stdout_sink(),
        plugins,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
    .expect("the build must succeed");
    take_cli_problems()
}

#[test]
fn a_text_embed_with_no_plugin_is_a_problem_except_under_no_plugins() {
    let files = [("index.md", "# Home\n\n![[docs/a.pdf#text]]\n"), ("docs/a.pdf", "bytes")];
    assert_eq!(build_once(&files, PluginMode::Skip), 0, "--no-plugins must not count a missing plugin's text");
    assert_eq!(build_once(&files, PluginMode::Blocking), 1, "with plugins on, the same embed is one problem");
}
