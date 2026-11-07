//! Pins an invariant: the build never takes the editor's no-map answer.
//!
//! The editor resolves a link on a site with no URL map to the page's source
//! file, so it can open it before any build has run. The build resolves links
//! on its own, so on a site's first build a link to a page that has never been
//! built still gets the page's URL, never a path to its `.md` source.

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
fn a_first_build_links_fresh_pages_by_url_not_by_source_file() {
    let tmp = std::env::temp_dir().join(format!("moss_unbuilt_link_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", "---\ntitle: Harbor Notes\n---\n\nSee [[Fresh]], [[Fresh#Tides]], [[notes/Deep]] and [the log](notes/Deep.md).\n");
    write(&tmp, "Fresh.md", "---\ntitle: Fresh\n---\n\n## Tides\n\nHigh at noon.\n");
    write(&tmp, "notes/Deep.md", "---\ntitle: Deep\n---\n\nBelow.\n");
    assert!(!tmp.join(".moss/build.nosync").exists(), "the site has never been built");
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");

    let html = fs::read_to_string(tmp.join(".moss/build.nosync/staging/index.html")).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse("main a[href]").unwrap();
    let hrefs: Vec<&str> = doc.select(&sel).filter_map(|a| a.value().attr("href")).collect();
    assert!(hrefs.len() >= 4, "every link is drawn: {hrefs:?}\n{html}");
    for href in &hrefs {
        let path = href.split('#').next().unwrap();
        assert!(!path.to_lowercase().ends_with(".md"), "a link names a source file: {href} in {hrefs:?}");
    }
    assert!(hrefs.iter().any(|h| h.ends_with("#tides")), "the anchor survives: {hrefs:?}");
}
