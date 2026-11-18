//! A places root, synthesized from the term index when the site has no
//! `places/index.md` or authored when it has one: the explorer is the page
//! when it is on, and the root is an ordinary folder page when it is off.

use std::fs;
use std::path::{Path, PathBuf};
use scraper::{Html, Selector};

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
        rename_pairs: Vec::new(),
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

fn extract_attribute(html: &str, attr_name: &str) -> Option<String> {
    let attr_prefix = format!("{}=\"", attr_name);
    if let Some(start_idx) = html.find(&attr_prefix) {
        let value_start = start_idx + attr_prefix.len();
        if let Some(end_idx) = html[value_start..].find('"') {
            return Some(html[value_start..value_start + end_idx].to_string());
        }
    }
    None
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// No `places/index.md` anywhere in this fixture: the `places` namespace
/// root is reached only through a post's `location:` field, so `blocking.rs`
/// synthesizes the folder and its index page from the term index.
#[test]
fn synthetic_places_root_gets_the_explorer_bundle_and_handshake() {
    let tmp = std::env::temp_dir().join(format!("moss_places_synthetic_root_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n");
    write(&tmp, ".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nA single place, named through `location:`.\n");

    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = tmp.join(".moss/build.nosync/staging");

    let places_html = read_page(&out, "places/index.html");
    let document = Html::parse_document(&places_html);
    let explorer_script = Selector::parse("script[src^='/_moss/js/places-explorer.']").unwrap();
    assert!(
        document.select(&explorer_script).next().is_some(),
        "synthetic places root must load the places-explorer bundle, got:\n{places_html}"
    );
    assert!(
        places_html.contains("data-moss-places-explorer"),
        "synthetic places root's map must carry the explorer handshake attributes, got:\n{places_html}"
    );
    assert!(places_html.contains("data-world=\""), "{places_html}");
    assert!(places_html.contains("data-tiles=\""), "{places_html}");
    assert!(places_html.contains("data-places=\""), "{places_html}");
    assert!(places_html.contains("data-scope=\"places\""), "{places_html}");

    let places_url = extract_attribute(&places_html, "data-places")
        .expect("data-places attribute must have a value");
    let places_path = out.join(places_url.trim_start_matches('/'));
    assert!(places_path.exists(), "data-places URL {} must point to an existing file at {}", places_url, places_path.display());

    let world_url = extract_attribute(&places_html, "data-world")
        .expect("data-world attribute must have a value");
    let world_path = out.join(world_url.trim_start_matches('/'));
    assert!(world_path.exists(), "data-world URL {} must point to an existing file at {}", world_url, world_path.display());

    let tiles_url = extract_attribute(&places_html, "data-tiles")
        .expect("data-tiles attribute must have a value");
    let tiles_path = out.join(tiles_url.trim_start_matches('/'));
    assert!(tiles_path.exists(), "data-tiles URL {} must point to an existing file at {}", tiles_url, tiles_path.display());

    // Design decision 7, "the map is the page", applies to a synthetic root
    // exactly as it does to an authored one: no visible heading, the map
    // directly under the site header. A site whose places root is declared
    // only in `.moss/config.toml` (no `places/index.md`) must get the same
    // `<h1 class="… visually-hidden">` treatment as the authored-root gate
    // fixture, not a plain visible folder title.
    let main_start = places_html.find("<main").expect("page has a <main>");
    let main = &places_html[main_start..];
    assert_eq!(main.matches("<h1").count(), 1, "the synthetic explorer root must carry exactly one <h1> in main: {places_html}");
    let h1_start = main.find("<h1").unwrap();
    let h1_end = main[h1_start..].find("</h1>").unwrap() + h1_start + "</h1>".len();
    let h1 = &main[h1_start..h1_end];
    assert!(h1.contains("visually-hidden"), "the synthetic explorer root's own <h1> must be visually hidden: {h1}");
}

/// `explorer = false` on the place-typed kind must suppress both the bundle
/// and the handshake on a synthetic root exactly as it does on a real one —
/// no script tag, no `data-moss-places-explorer`.
#[test]
fn explorer_false_leaves_a_synthetic_root_untouched() {
    let tmp = std::env::temp_dir().join(format!("moss_places_synthetic_root_off_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\nexplorer = false\n");
    write(&tmp, ".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nA single place, named through `location:`.\n");

    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let out = tmp.join(".moss/build.nosync/staging");

    let places_html = read_page(&out, "places/index.html");
    assert!(
        !places_html.contains("places-explorer"),
        "explorer = false must suppress the bundle on a synthetic root too, got:\n{places_html}"
    );
    assert!(
        !places_html.contains("data-moss-places-explorer"),
        "explorer = false must suppress the handshake on a synthetic root too, got:\n{places_html}"
    );
    let main = &places_html[places_html.find("<main").expect("page has a <main>")..];
    let h1 = &main[main.find("<h1").expect("an <h1>")..];
    assert!(!h1[..h1.find("</h1>").unwrap()].contains("visually-hidden"), "the title shows when there is no explorer:\n{main}");
    assert!(main.find("<h1") < main.find("moss-place-map"), "the title comes before the map:\n{main}");
    assert!(main.contains(r#"href="/places/harbor/""#), "the root lists its places:\n{main}");
}

/// With the explorer off, a places root that is a real page of its own is an
/// ordinary folder page: its title shows, and its words come before its map.
#[test]
fn explorer_false_keeps_an_authored_roots_title_and_puts_its_words_first() {
    let tmp = std::env::temp_dir().join(format!("moss_places_authored_root_off_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());

    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\nexplorer = false\n");
    write(&tmp, ".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
    write(&tmp, "places/index.md", "---\ntitle: Where it happened\n---\n\nThe words about the map.\n");
    write(&tmp, "posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nA single place, named through `location:`.\n");

    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    let places_html = read_page(&tmp.join(".moss/build.nosync/staging"), "places/index.html");
    let main = &places_html[places_html.find("<main").expect("page has a <main>")..];
    let h1 = &main[main.find("<h1").expect("an <h1>")..];
    let h1 = &h1[..h1.find("</h1>").unwrap()];
    assert!(!h1.contains("visually-hidden"), "the title shows when there is no explorer: {h1}");
    let words = main.find("The words about the map").expect("the page's words");
    let map = main.find("moss-place-map").expect("the page's map");
    assert!(words < map, "the words come before the map:\n{main}");
    assert!(main.contains(r#"href="/places/harbor/""#), "the root lists its places:\n{main}");
}

/// A root page that already draws the site's map draws it once, even when its
/// words embed the same map: the embed adds nothing there, with the explorer on
/// or off.
#[test]
fn a_root_that_embeds_its_own_map_draws_it_once() {
    for explorer in [true, false] {
        let tmp = std::env::temp_dir().join(format!("moss_places_root_embed_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let _cleanup = Cleanup(tmp.clone());

        write(&tmp, ".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\nexplorer = {explorer}\n"));
        write(&tmp, ".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n");
        write(&tmp, "index.md", "---\ntitle: Home\n---\n\nHi.\n");
        write(&tmp, "places/index.md", "---\ntitle: Where it happened\n---\n\nThe words about the map.\n\n![[/places/|style:map]]\n");
        write(&tmp, "posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nA single place, named through `location:`.\n");

        let result = build_sync(&tmp.to_string_lossy());
        assert!(result.is_ok(), "build failed: {result:?}");
        let places_html = read_page(&tmp.join(".moss/build.nosync/staging"), "places/index.html");
        let main = &places_html[places_html.find("<main").expect("page has a <main>")..];
        assert_eq!(main.matches(r#"class="moss-place-map""#).count(), 1, "explorer = {explorer}: one map:\n{main}");
        if explorer {
            assert!(!main.contains("moss-cards"), "the embed draws no listing in the map's place:\n{main}");
        }
    }
}
