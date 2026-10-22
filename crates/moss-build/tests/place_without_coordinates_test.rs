//! A gazetteer place with no `lat`/`lng` is advice, not a failure: a page that
//! names only that place still builds, and it simply gets no map.

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

#[test]
fn a_page_whose_only_place_has_no_coordinates_builds_without_a_map() {
    let tmp = std::env::temp_dir().join(format!("moss_place_no_coords_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
    write(&tmp, ".moss/places.toml", "[\"Nowhere\"]\nprecision = \"city\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/lost.md", "---\ntitle: Lost\nlocation: Nowhere\n---\n\nBody.\n");

    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = tmp.join(".moss/build.nosync/staging");
    let page = fs::read_to_string(out.join("posts/lost/index.html")).expect("the page still builds");
    assert!(page.contains("Nowhere"), "the page still names its place");
    let place = fs::read_to_string(out.join("places/nowhere/index.html")).expect("the place page still builds");
    assert!(!place.contains("moss-place-map"), "no map for a place without coordinates");
}
