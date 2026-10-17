//! A moss-hosted build writes the visit counter's one script file, and every
//! page loads it with one tag. A site without a site id gets neither.

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

struct Site(PathBuf);

impl Site {
    fn new(state: Option<&str>) -> Self {
        let dir = std::env::temp_dir().join(format!("moss_beacon_file_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join(".moss")).unwrap();
        fs::write(dir.join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
        if let Some(state) = state {
            fs::write(dir.join(".moss/state.toml"), state).unwrap();
        }
        Site(dir)
    }

    fn build(&self) -> PathBuf {
        let result = build_sync(&self.0.to_string_lossy());
        assert!(result.is_ok(), "build failed: {result:?}");
        self.0.join(".moss/build.nosync/staging")
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read(out: &Path, rel: &str) -> String {
    fs::read_to_string(out.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn moss_hosted_build_writes_the_beacon_file_and_the_page_loads_it() {
    let site = Site::new(Some("[deployment]\nsite_id = \"blog\"\ndeploy_method = \"moss\"\n"));
    let out = site.build();

    let js = read(&out, "_moss/js/beacon.js");
    assert!(js.contains("/api/sites/blog/beacon\""), "endpoint missing from:\n{js}");
    assert!(
        read(&out, "index.html").contains(r#"<script data-moss-deploy-only src="/_moss/js/beacon.js" defer></script>"#),
        "the home page must load the beacon file with the deploy-only tag"
    );
}

#[test]
fn site_without_a_site_id_writes_no_beacon_file() {
    let site = Site::new(Some("[deployment]\ndeploy_method = \"moss\"\n"));
    let out = site.build();

    assert!(!out.join("_moss/js/beacon.js").exists());
    assert!(!read(&out, "index.html").contains("beacon.js"));
}
