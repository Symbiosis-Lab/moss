//! The place line's `line = false` switch on a place-typed kind: the line
//! disappears from every located page, while `location:` keeps feeding
//! listing cards and the article locator.

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

fn read_page(out: &Path, url: &str) -> String {
    fs::read_to_string(out.join(url)).unwrap_or_else(|e| panic!("{url}: {e}"))
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const PLACES: &str = "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n";

/// Build a one-article site and return (article html, listing html).
fn build_site(terms_extra: &str, site_extra: &str, article_fm: &str) -> (String, String) {
    let tmp = std::env::temp_dir().join(format!("moss_place_line_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(
        &tmp,
        ".moss/config.toml",
        &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n{site_extra}\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n{terms_extra}"),
    );
    write(&tmp, ".moss/places.toml", PLACES);
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/report.md", &format!("---\ntitle: Report\ndate: 2025-05-22\nlocation: Harbor\n{article_fm}---\n\nA single place.\n"));
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = tmp.join(".moss/build.nosync/staging");
    (read_page(&out, "posts/report/index.html"), read_page(&out, "posts/index.html"))
}

#[test]
fn the_place_line_renders_by_default() {
    let (article, _) = build_site("", "locator = \"align-right\"\n", "");
    assert!(article.contains("moss-place-line"), "{article}");
}

#[test]
fn line_false_drops_the_line_but_keeps_card_meta_and_locator() {
    let (article, listing) = build_site("line = false\n", "locator = \"align-right\"\n", "");
    assert!(!article.contains("moss-place-line"), "{article}");
    assert!(article.contains("moss-place-locator"), "locator must still show: {article}");
    assert!(listing.contains("moss-card-meta") && listing.contains("Harbor"), "card meta must still name the place: {listing}");
}
