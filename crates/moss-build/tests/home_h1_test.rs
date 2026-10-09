//! The home page needs exactly one top-level heading. A home whose body has no
//! heading of its own gets a visually hidden one carrying the page title; a
//! home that opens with `# Title` or a hero that draws its own heading already
//! has one, and adds none.

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

fn home_html(home_body: &str) -> String {
    let tmp = std::env::temp_dir().join(format!("moss_home_h1_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", &format!("---\ntitle: Harbor Notes\n---\n\n{home_body}\n"));
    write(&tmp, "posts/first.md", "---\ntitle: First Post\n---\n\nHello.\n");
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    fs::read_to_string(tmp.join(".moss/build.nosync/staging/index.html")).unwrap()
}

fn h1s(html: &str) -> Vec<String> {
    let doc = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse("h1").unwrap();
    doc.select(&sel)
        .map(|e| format!("{}|{}", e.value().attr("class").unwrap_or(""), e.text().collect::<String>()))
        .collect()
}

#[test]
fn home_without_a_body_heading_gets_a_visually_hidden_h1() {
    let html = home_html("Welcome to the harbor.");
    assert_eq!(h1s(&html), vec!["moss-folder-title visually-hidden|Harbor Notes"], "{html}");
}

#[test]
fn home_opening_with_its_own_h1_adds_none() {
    let html = home_html("# Welcome\n\nHello.");
    assert_eq!(h1s(&html), vec!["|Welcome"], "{html}");
}

#[test]
fn home_whose_hero_draws_a_heading_adds_none() {
    let html = home_html(":::hero\n# Overlay Title\n:::\n\nHello.");
    assert!(!html.contains("moss-folder-title"), "{html}");
    assert_eq!(h1s(&html).len(), 1, "{html}");
}

#[test]
fn home_opening_with_an_image_then_its_own_h1_adds_none() {
    let html = home_html("![cover](cover.jpg)\n\n# Welcome\n\nHello.");
    assert_eq!(h1s(&html), vec!["|Welcome"], "{html}");
}

#[test]
fn home_whose_first_heading_is_an_h2_still_gets_the_hidden_h1() {
    let html = home_html("## Latest\n\nHello.");
    assert_eq!(h1s(&html), vec!["moss-folder-title visually-hidden|Harbor Notes"], "{html}");
}
