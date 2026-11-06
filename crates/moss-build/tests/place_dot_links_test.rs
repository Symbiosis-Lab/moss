//! The dots of a static place map link to their place pages, built through
//! the real pipeline: the article locator, a place's own page, and the places
//! root with the interactive explorer off.

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

const PLACES: &str = "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n\n[\"Harbor Annex\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n\n[\"Inland\"]\nlat = 36.0\nlng = 136.0\nprecision = \"city\"\n";

/// Build a three-article site (`line = false`, no explorer) and return its output folder.
fn build_site(tmp: &Path) {
    write(tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\nlocator = \"align-right\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\nline = false\nexplorer = false\n");
    write(tmp, ".moss/places.toml", PLACES);
    write(tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(tmp, "posts/report.md", "---\ntitle: Report\ndate: 2025-05-22\nlocation: Harbor\n---\n\nA single place.\n");
    write(tmp, "posts/annex.md", "---\ntitle: Annex\ndate: 2025-05-23\nlocation: Harbor Annex\n---\n\nSame spot.\n");
    write(tmp, "posts/trip.md", "---\ntitle: Trip\ndate: 2025-05-24\nlocation: Inland\n---\n\nFar.\n");
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
}

fn built<R>(check: impl FnOnce(&Path) -> R) -> R {
    let tmp = std::env::temp_dir().join(format!("moss_place_dots_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    build_site(&tmp);
    check(&tmp.join(".moss/build.nosync/staging"))
}

fn figure(html: &str) -> &str {
    let class = html.find("class=\"moss-place-map\"").expect("a place map");
    let start = html[..class].rfind("<figure").unwrap();
    &html[start..start + html[start..].find("</figure>").unwrap()]
}

#[test]
fn a_locator_dot_links_to_its_place_page_and_the_figure_is_not_an_image() {
    built(|out| {
        let map = read_page(out, "posts/trip/index.html");
        let map = figure(&map);
        assert!(map.contains("<a href=\"/places/inland/\" aria-label=\"Inland\">"), "{map}");
        assert!(!map.contains("role=\"img\""), "a figure holding links cannot be role=img: {map}");
        assert!(map.contains("aria-label=\"Map of Inland"), "{map}");
        let svg = &map[map.find("<svg ").unwrap()..];
        assert!(!svg[..svg.find('>').unwrap()].contains("aria-hidden"), "links inside an aria-hidden svg are unreachable");
        assert!(map.contains("stroke=\"transparent\""), "the 24px hit target: {map}");
    });
}

#[test]
fn a_places_own_page_does_not_link_its_own_dot() {
    built(|out| {
        let page = read_page(out, "places/inland/index.html");
        let map = figure(&page);
        assert!(!map.contains("<a "), "{map}");
        assert!(map.contains("role=\"img\""), "{map}");
    });
}

#[test]
fn the_static_places_root_links_each_dot_and_a_cluster_names_what_it_holds() {
    built(|out| {
        let page = read_page(out, "places/index.html");
        let map = figure(&page);
        assert!(map.contains("<a href=\"/places/inland/\" aria-label=\"Inland\">"), "{map}");
        assert!(map.contains("aria-label=\"Harbor, Harbor Annex\""), "the coincident places are one dot: {map}");
        assert_eq!(map.matches("<a ").count(), 2, "{map}");
    });
}
