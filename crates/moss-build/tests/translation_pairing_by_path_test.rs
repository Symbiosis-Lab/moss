//! A page and its translation in a language folder pair by their path inside
//! the language tree; a shared `translationKey:` pairs pages whatever they are
//! called. A folder that does not name a language is just a folder.

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

/// Build a site from `files` (path, frontmatter extras) and return a reader
/// for built pages, relative to the staging directory.
fn build(files: &[(&str, &str)]) -> (PathBuf, Cleanup) {
    let tmp = std::env::temp_dir().join(format!("moss_pairing_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", "---\ntitle: Harbor Notes\n---\n\nWelcome.\n");
    for (rel, extra) in files {
        write(&tmp, rel, &format!("---\ntitle: Page {rel}\n{extra}---\n\nBody.\n"));
    }
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    (tmp, cleanup)
}

/// The `href` of every language link in the built page's navigation.
fn alternates(site: &Path, page: &str) -> Vec<String> {
    let html = fs::read_to_string(site.join(".moss/build.nosync/staging").join(page))
        .unwrap_or_else(|e| panic!("{page}: {e}"));
    let doc = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse("a.nav-lang-link").unwrap();
    let mut hrefs: Vec<String> = doc
        .select(&sel)
        .filter_map(|e| e.value().attr("href").map(String::from))
        .collect();
    hrefs.sort();
    hrefs
}

fn links_to(site: &Path, page: &str, target: &str) -> bool {
    alternates(site, page).iter().any(|h| h.trim_end_matches('/').ends_with(target))
}

#[test]
fn same_name_in_a_language_folder_pairs_without_a_key() {
    let (site, _c) = build(&[("x.md", ""), ("de/index.md", ""), ("de/x.md", "")]);
    assert!(links_to(&site, "x/index.html", "/de/x"), "{:?}", alternates(&site, "x/index.html"));
    assert!(links_to(&site, "de/x/index.html", "/x"), "{:?}", alternates(&site, "de/x/index.html"));
}

#[test]
fn nested_pages_pair_only_on_the_same_path_inside_the_tree() {
    let (site, _c) = build(&[("a/x.md", ""), ("de/index.md", ""), ("de/a/x.md", ""), ("de/b/x.md", "")]);
    assert!(links_to(&site, "a/x/index.html", "/de/a/x"), "{:?}", alternates(&site, "a/x/index.html"));
    assert!(!links_to(&site, "a/x/index.html", "/de/b/x"), "{:?}", alternates(&site, "a/x/index.html"));
    assert!(alternates(&site, "de/b/x/index.html").iter().all(|h| !h.trim_end_matches('/').ends_with("/a/x")));
}

#[test]
fn a_folder_that_is_not_a_language_does_not_pair() {
    let (site, _c) = build(&[("x.md", ""), ("docs/x.md", "")]);
    assert!(alternates(&site, "x/index.html").is_empty(), "{:?}", alternates(&site, "x/index.html"));
    assert!(alternates(&site, "docs/x/index.html").is_empty());
}

#[test]
fn a_shared_translation_key_pairs_pages_with_different_names() {
    let (site, _c) = build(&[
        ("harbor.md", "translationKey: harbor\n"),
        ("de/index.md", ""),
        ("de/hafen.md", "translationKey: harbor\n"),
    ]);
    assert!(links_to(&site, "harbor/index.html", "/de/hafen"), "{:?}", alternates(&site, "harbor/index.html"));
    assert!(links_to(&site, "de/hafen/index.html", "/harbor"), "{:?}", alternates(&site, "de/hafen/index.html"));
}

#[test]
fn a_paired_home_keeps_the_generated_folder_page_and_pairs_pages_by_path() {
    let (site, _c) = build(&[("guide/x.md", ""), ("de/index.md", ""), ("de/guide/x.md", "")]);
    assert!(site.join(".moss/build.nosync/staging/de/guide/index.html").exists());
    assert!(links_to(&site, "guide/x/index.html", "/de/guide/x"), "{:?}", alternates(&site, "guide/x/index.html"));
}

#[test]
fn an_authored_key_outranks_a_path_twin_in_the_same_language() {
    let (site, _c) = build(&[
        ("x.md", "translationKey: k\n"),
        ("de/index.md", ""),
        ("de/x.md", ""),
        ("de/y.md", "translationKey: k\n"),
    ]);
    assert_eq!(alternates(&site, "x/index.html").len(), 1, "{:?}", alternates(&site, "x/index.html"));
    assert!(links_to(&site, "x/index.html", "/de/y"));
    let html = fs::read_to_string(site.join(".moss/build.nosync/staging/x/index.html")).unwrap();
    assert_eq!(html.matches(r#"hreflang="de""#).count(), 1, "{html}");
}

#[test]
fn a_keyed_page_still_pairs_with_a_keyless_twin_in_another_language() {
    let (site, _c) = build(&[
        ("x.md", "translationKey: k\n"),
        ("de/index.md", ""),
        ("de/y.md", "translationKey: k\n"),
        ("x.zh-hans.md", ""),
    ]);
    assert!(links_to(&site, "x/index.html", "/de/y"), "{:?}", alternates(&site, "x/index.html"));
    assert_eq!(alternates(&site, "x/index.html").len(), 2, "{:?}", alternates(&site, "x/index.html"));
}
