//! The place line's `line = false` switch on a place-typed kind: the line
//! disappears from every located page, while `location:` keeps feeding
//! listing cards and the article locator. Also the page's own `map:` switch,
//! which turns that page's locator on or off against the site's setting.

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

const PLACES: &str = "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n\n[\"Inland\"]\nlat = 36.0\nlng = 136.0\nprecision = \"city\"\n";

/// Build a one-article site and return (article html, listing html).
fn build_site(terms_extra: &str, site_extra: &str, article_fm: &str) -> (String, String) {
    build_located_site(terms_extra, site_extra, article_fm, "Harbor")
}

fn build_located_site(terms_extra: &str, site_extra: &str, article_fm: &str, location: &str) -> (String, String) {
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
    write(&tmp, "posts/report.md", &format!("---\ntitle: Report\ndate: 2025-05-22\nlocation: {location}\n{article_fm}---\n\nA single place.\n"));
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

#[test]
fn page_map_true_shows_the_locator_where_the_site_has_none() {
    let (article, _) = build_site("", "locator = \"none\"\n", "map: true\n");
    assert!(article.contains("moss-place-locator"), "{article}");
}

#[test]
fn page_map_false_hides_the_locator_where_the_site_has_one() {
    let (article, _) = build_site("", "locator = \"align-right\"\n", "map: false\n");
    assert!(!article.contains("moss-place-locator"), "{article}");
    assert!(article.contains("moss-place-line"), "the place line is not the map: {article}");
}

#[test]
fn an_unset_page_map_follows_the_site_locator_either_way() {
    let (on, _) = build_site("", "locator = \"align-right\"\n", "");
    let (off, _) = build_site("", "locator = \"none\"\n", "");
    assert!(on.contains("moss-place-locator"));
    assert!(!off.contains("moss-place-locator"));
}

#[test]
fn page_map_true_with_no_resolved_location_renders_nothing_and_builds() {
    let tmp = std::env::temp_dir().join(format!("moss_place_line_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\nlocator = \"none\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
    write(&tmp, ".moss/places.toml", PLACES);
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/none.md", "---\ntitle: None\nmap: true\n---\n\nNo location.\n");
    write(&tmp, "posts/gone.md", "---\ntitle: Gone\nmap: true\nlocation: Nowhere\n---\n\nUnknown place.\n");
    write(&tmp, "posts/bare.md", "---\ntitle: Bare\nmap: true\nlocation: Harbor\n---\n\nKnown.\n");
    assert!(build_sync(&tmp.to_string_lossy()).is_ok());
    let out = tmp.join(".moss/build.nosync/staging");
    assert!(!read_page(&out, "posts/none/index.html").contains("moss-place-locator"));
    assert!(!read_page(&out, "posts/gone/index.html").contains("moss-place-locator"));
    assert!(read_page(&out, "posts/bare/index.html").contains("moss-place-locator"));
}

/// A claimed place page keeps `map: false`'s existing meaning: its own term
/// map is off. Without the key the same page draws it.
#[test]
fn map_false_on_a_place_page_still_turns_its_term_map_off() {
    let build = |fm: &str| {
        let tmp = std::env::temp_dir().join(format!("moss_place_line_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let _cleanup = Cleanup(tmp.clone());
        write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
        write(&tmp, ".moss/places.toml", PLACES);
        write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
        write(&tmp, "harbor.md", &format!("---\ntitle: Harbor\nplace_page: true\n{fm}---\n\nThe harbor.\n"));
        write(&tmp, "posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nText.\n");
        assert!(build_sync(&tmp.to_string_lossy()).is_ok());
        read_page(&tmp.join(".moss/build.nosync/staging"), "harbor/index.html")
    };
    assert!(build("").contains("moss-place-map"));
    assert!(!build("map: false\n").contains("moss-place-map"));
}

#[test]
fn route_true_and_map_true_draw_the_route_on_a_locator_the_site_did_not_ask_for() {
    let (article, _) = build_located_site(
        "",
        "locator = \"none\"\n",
        "map: true\nroute: true\n",
        "[Harbor, Inland]",
    );
    assert!(article.contains("moss-place-locator"), "{article}");
    assert!(article.contains("data-map-route=\"line\""), "route must draw on the page's map: {article}");
}

/// A place page's own map is its term map; it never also gets a locator,
/// even with `location:` and `map: true`.
#[test]
fn a_claimed_place_page_never_gets_a_locator() {
    let tmp = std::env::temp_dir().join(format!("moss_place_line_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\nlocator = \"align-right\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
    write(&tmp, ".moss/places.toml", PLACES);
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "harbor.md", "---\ntitle: Harbor\nplace_page: true\nlocation: Harbor\nmap: true\n---\n\nThe harbor.\n");
    assert!(build_sync(&tmp.to_string_lossy()).is_ok());
    let page = read_page(&tmp.join(".moss/build.nosync/staging"), "harbor/index.html");
    assert!(page.contains("moss-place-map"), "its own term map still draws: {page}");
    assert!(!page.contains("moss-place-locator"), "{page}");
}

#[test]
fn place_pages_omit_geographic_parent_annotations_but_keep_authored_text_and_hierarchy() {
    let tmp = std::env::temp_dir().join(format!("moss_place_annotations_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
    write(&tmp, ".moss/places.toml", "[\"Village\"]\nlat = 10.0\nlng = 20.0\nprecision = \"city\"\nparent = \"Region\"\n\n[\"Region\"]\nlat = 12.0\nlng = 22.0\nprecision = \"region\"\nparent = \"Territory\"\n\n[\"Territory\"]\nlat = 15.0\nlng = 25.0\nprecision = \"country\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\nbreadcrumb: true\n---\n\nHi.\n");
    write(&tmp, "village.md", "---\ntitle: Village\nplace_page: true\n---\n\nAn authored mention of Territory.\n");
    write(&tmp, "posts/report.md", "---\ntitle: Field report\nlocation: Village\n---\n\nText.\n");
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = tmp.join(".moss/build.nosync/staging");
    let claimed = read_page(&out, "village/index.html");
    let generated = read_page(&out, "places/region/index.html");
    for html in [&claimed, &generated] {
        assert!(!html.contains("moss-place-breadcrumb"), "parent annotation must not render");
        assert!(html.contains("Field report"), "rolled-up works must remain");
        assert!(html.contains("class=\"site-name\""), "normal home navigation must remain");
    }
    assert!(claimed.contains("An authored mention of Territory."));
    assert!(generated.contains("moss-place-children"));
    assert!(generated.contains("Village (1)"));
    let places_path = fs::read_dir(out.join("_moss")).unwrap().map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with("places.") && path.extension().is_some_and(|ext| ext == "json"))
        .expect("explorer places data");
    let data: serde_json::Value = serde_json::from_str(&fs::read_to_string(places_path).unwrap()).unwrap();
    let village = data["places"].as_array().unwrap().iter().find(|place| place["id"] == "places/village").unwrap();
    assert_eq!(village["parent"], "places/region", "internal geographic hierarchy must remain");
}
