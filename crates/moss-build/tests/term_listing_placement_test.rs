//! A claimed term page lists its pages after its body, except that a body
//! ending in a sources section (`## Sources`, `## Quellen`, ...) gets the
//! listing before that section.

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

/// Build a site whose `acme.md` claims the "Acme" partner term with the given
/// frontmatter extras and body, and return that page's HTML.
fn claimed_page(extra_fm: &str, body: &str) -> String {
    let tmp = std::env::temp_dir().join(format!("moss_term_placement_{}", uuid::Uuid::new_v4()));
    let write = |rel: &str, text: &str| {
        let path = tmp.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.partners]\nfields = [\"partner\"]\n");
    write("index.md", "---\ntitle: Home\n---\n\nHello.\n");
    write("acme.md", &format!("---\ntitle: Acme\npartner_page: true\n{extra_fm}---\n\n{body}\n"));
    write("notes/first.md", "---\ntitle: First Note\npartner: [Acme]\n---\n\nNote.\n");
    let result = build_sync(&tmp.to_string_lossy());
    let html = fs::read_to_string(tmp.join(".moss/build.nosync/staging/acme/index.html"));
    let _ = fs::remove_dir_all(&tmp);
    assert!(result.is_ok(), "build failed: {result:?}");
    html.unwrap()
}

fn at(html: &str, needle: &str) -> usize {
    html.find(needle).unwrap_or_else(|| panic!("{needle} not in {html}"))
}

const LISTING: &str = "First Note";

#[test]
fn the_listing_goes_before_a_closing_sources_section() {
    let html = claimed_page("", "Intro.\n\n## History\n\nLong ago.\n\n## Sources\n\n- Source entry\n");
    assert!(at(&html, "Long ago.") < at(&html, LISTING), "history stays above the listing");
    assert!(at(&html, LISTING) < at(&html, r#"id="sources""#), "listing precedes the heading");
    assert!(at(&html, r#"id="sources""#) < at(&html, "Source entry"), "sources list stays under its heading");
}

#[test]
fn a_body_without_a_sources_heading_keeps_the_listing_after_it() {
    let html = claimed_page("", "Intro.\n\n## History\n\nLong ago.\n");
    assert!(at(&html, "Long ago.") < at(&html, LISTING));
}

#[test]
fn a_sources_section_that_is_not_last_does_not_move_the_listing() {
    let html = claimed_page("", "Intro.\n\n## Sources\n\n- Source entry\n\n## Notes\n\nClosing words.\n");
    assert!(at(&html, "Closing words.") < at(&html, LISTING));
}

#[test]
fn the_heading_is_compared_without_regard_to_case() {
    let html = claimed_page("", "Intro.\n\n## REFERENCES\n\n- Source entry\n");
    assert!(at(&html, LISTING) < at(&html, r#"id="references""#));
}

#[test]
fn a_german_page_closes_on_quellen() {
    let html = claimed_page("lang: de\n", "Einleitung.\n\n## Quellen\n\n- Quelle eins\n");
    assert!(at(&html, LISTING) < at(&html, r#"id="quellen""#));
    assert!(at(&html, r#"id="quellen""#) < at(&html, "Quelle eins"));
}

#[test]
fn a_chinese_page_closes_on_its_own_sources_heading() {
    let html = claimed_page("lang: zh-hans\n", "简介。\n\n## 来源\n\n- 条目一\n");
    assert!(at(&html, LISTING) < at(&html, "条目一"));
    assert!(at(&html, LISTING) < at(&html, ">来源<"));
    assert!(at(&html, ">来源<") < at(&html, "条目一"));
}
