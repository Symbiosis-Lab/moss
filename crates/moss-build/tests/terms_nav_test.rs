//! `nav = true` in a `[terms.<key>]` block puts the kind's root page in the
//! nav bar, after the content entries, in the order the kinds appear in the
//! config. Without it the nav is what it was.

use std::fs;
use std::path::PathBuf;

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

/// Build a site with the given `[terms]` config and return the nav links
/// (`href`, text) of its home page.
fn nav_links(terms_config: &str) -> Vec<(String, String)> {
    nav_links_with(terms_config, &[])
}

/// As [`nav_links`], with extra root pages given as `(file, frontmatter lines)`.
fn nav_links_with(terms_config: &str, pages: &[(&str, &str)]) -> Vec<(String, String)> {
    let tmp = std::env::temp_dir().join(format!("moss_terms_nav_{}", uuid::Uuid::new_v4()));
    let write = |rel: &str, body: &str| {
        let path = tmp.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    };
    write(".moss/config.toml", &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n\n{terms_config}"));
    write("index.md", "---\ntitle: Home\n---\n\nHello.\n");
    write("about.md", "---\ntitle: About\nnav: true\n---\n\nAbout.\n");
    write(
        "notes/first.md",
        "---\ntitle: First\npartner: [Acme]\nsponsor: [Birch]\n---\n\nNote.\n",
    );
    for (file, front) in pages {
        write(file, &format!("---\n{front}\nnav: true\n---\n\nBody.\n"));
    }
    let result = build_sync(&tmp.to_string_lossy());
    let html = fs::read_to_string(tmp.join(".moss/build.nosync/staging/index.html"));
    let _ = fs::remove_dir_all(&tmp);
    assert!(result.is_ok(), "build failed: {result:?}");
    let html = html.unwrap();
    let doc = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse("#nav-links a").unwrap();
    doc.select(&sel)
        .map(|a| (a.value().attr("href").unwrap_or("").to_string(), a.text().collect::<String>()))
        .collect()
}

fn link(href: &str, text: &str) -> (String, String) {
    (href.to_string(), text.to_string())
}

#[test]
fn a_kind_with_nav_true_is_a_nav_link_after_the_content_entries() {
    let links = nav_links("[terms.partners]\nfields = [\"partner\"]\nnav = true\n");
    assert_eq!(links, vec![link("/about/", "About"), link("/partners/", "Partners")]);
}

#[test]
fn the_kinds_title_labels_the_link() {
    let links = nav_links("[terms.partners]\nfields = [\"partner\"]\ntitle = \"Our partners\"\nnav = true\n");
    assert_eq!(links.last(), Some(&link("/partners/", "Our partners")));
}

#[test]
fn a_kind_without_nav_leaves_the_nav_as_it_was() {
    let links = nav_links("[terms.partners]\nfields = [\"partner\"]\n");
    assert_eq!(links, vec![link("/about/", "About")]);
}

#[test]
fn kinds_keep_their_config_order() {
    let links = nav_links(
        "[terms.sponsors]\nfields = [\"sponsor\"]\nnav = true\n\n[terms.partners]\nfields = [\"partner\"]\nnav = true\n",
    );
    let hrefs: Vec<&str> = links.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(hrefs, vec!["/about/", "/sponsors/", "/partners/"], "config order, not alphabetical: {links:?}");
}

const WEIGHTED_PAGES: [(&str, &str); 2] = [("one.md", "title: One\nweight: 1"), ("three.md", "title: Three\nweight: 3")];

#[test]
fn a_weighted_kind_sits_among_the_pages_by_weight() {
    let links = nav_links_with("[terms.partners]\nfields = [\"partner\"]\nnav = true\nweight = 2\n", &WEIGHTED_PAGES);
    let hrefs: Vec<&str> = links.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(hrefs, vec!["/one/", "/partners/", "/three/", "/about/"], "{links:?}");
}

#[test]
fn an_unweighted_kind_stays_after_every_page() {
    let links = nav_links_with("[terms.partners]\nfields = [\"partner\"]\nnav = true\n", &WEIGHTED_PAGES);
    let hrefs: Vec<&str> = links.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(hrefs, vec!["/one/", "/three/", "/about/", "/partners/"], "{links:?}");
}

#[test]
fn weighted_and_unweighted_kinds_mix() {
    let links = nav_links_with(
        "[terms.sponsors]\nfields = [\"sponsor\"]\nnav = true\n\n[terms.partners]\nfields = [\"partner\"]\nnav = true\nweight = 5\n",
        &WEIGHTED_PAGES,
    );
    let hrefs: Vec<&str> = links.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(hrefs, vec!["/one/", "/three/", "/partners/", "/about/", "/sponsors/"], "{links:?}");
}

#[test]
fn kinds_with_weights_order_by_weight_not_by_config() {
    let links = nav_links(
        "[terms.sponsors]\nfields = [\"sponsor\"]\nnav = true\nweight = 2\n\n[terms.partners]\nfields = [\"partner\"]\nnav = true\nweight = 1\n",
    );
    let hrefs: Vec<&str> = links.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(hrefs, vec!["/partners/", "/sponsors/", "/about/"], "{links:?}");
}

#[test]
fn a_built_in_kind_without_a_title_uses_its_translated_title() {
    let links = nav_links_with("[terms.authors]\nnav = true\n", &[("ada.md", "title: Ada Piece\nauthor: [Ada]")]);
    assert_eq!(links.last(), Some(&link("/authors/", "Authors")), "{links:?}");
}

#[test]
fn a_declared_kind_without_a_title_is_its_key_capitalized() {
    let links = nav_links("[terms.sponsors]\nfields = [\"sponsor\"]\nnav = true\n");
    assert_eq!(links.last(), Some(&link("/sponsors/", "Sponsors")), "{links:?}");
}
