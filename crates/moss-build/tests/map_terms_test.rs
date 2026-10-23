//! Place-term fixtures: which field names feed a place term, how the term's root
//! embeds as a map, and what the build says when the home page and the nav both
//! show the same kind.

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

fn site(name: &str, terms: &str) -> (PathBuf, Cleanup) {
    let tmp = std::env::temp_dir().join(format!("moss_map_terms_{name}_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    write(&tmp, ".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n\n{terms}"));
    write(&tmp, ".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n");
    let cleanup = Cleanup(tmp.clone());
    (tmp, cleanup)
}

fn built(tmp: &Path) -> PathBuf {
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    tmp.join(".moss/build.nosync/staging")
}

#[test]
fn a_field_named_place_feeds_a_place_term_and_place_page_still_claims() {
    let (tmp, _c) = site("place_field", "[terms.places]\ntype = \"place\"\nfields = [\"place\"]\ntitle = \"Places\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/report.md", "---\ntitle: Report\nplace: Harbor\n---\n\nBody.\n");
    write(&tmp, "harbor.md", "---\ntitle: Harbor\nplace_page: true\n---\n\nThe claimed harbor page.\n");
    let out = built(&tmp);

    let harbor = fs::read_to_string(out.join("harbor/index.html")).expect("the claiming page");
    assert!(harbor.contains("The claimed harbor page."));
    assert!(harbor.contains("Report"), "the claiming page lists the pages naming the place");
    let generated = fs::read_to_string(out.join("places/harbor/index.html")).expect("generated URL");
    assert!(generated.contains("/harbor/"), "the generated URL forwards to the claiming page");
}

#[test]
fn the_place_root_embeds_as_a_map_in_another_page() {
    let (tmp, _c) = site("root_embed", "[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n\n![[/places/|style:map]]\n");
    write(&tmp, "posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nBody.\n");
    let out = built(&tmp);

    let home = fs::read_to_string(out.join("index.html")).unwrap();
    assert!(home.contains("moss-place-map"), "the home page draws the site's map");
    assert!(home.contains(r#"data-hydrate-url="/places/?embed=1""#), "and hydrates through the root's embed mode: {home}");
    assert!(!home.contains("moss-embed-missing"));
}

/// Build the same site with the place term fed by `field` and return the root,
/// a place page and an article with a `#` map embed.
fn maps_fed_by(field: &str) -> (String, String, String) {
    let (tmp, _c) = site(&format!("maps_{field}"), &format!("[terms.places]\ntype = \"place\"\nfields = [\"{field}\"]\ntitle = \"Places\"\n"));
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/report.md", &format!("---\ntitle: Report\n{field}: Harbor\n---\n\nBody.\n\n![[#|style:map]]\n"));
    let out = built(&tmp);
    let read = |p: &str| fs::read_to_string(out.join(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
    (read("places/index.html"), read("places/harbor/index.html"), read("posts/report/index.html"))
}

#[test]
fn a_place_term_fed_by_place_draws_the_same_maps_as_one_fed_by_location() {
    let (root, place, article) = maps_fed_by("place");
    let (l_root, l_place, l_article) = maps_fed_by("location");
    for (name, got, want) in [("root", &root, &l_root), ("place page", &place, &l_place), ("article embed", &article, &l_article)] {
        assert!(got.contains("data-map-marker=\"places/harbor\""), "{name} has no marker");
        assert_eq!(got.matches("data-map-marker=").count(), want.matches("data-map-marker=").count(), "{name}");
        assert_eq!(got.matches("moss-place-map").count(), want.matches("moss-place-map").count(), "{name}");
    }
}
