//! A site may declare its own name-list fields in `[terms.<key>] fields`.
//! A declared field feeds term pages like `author:` does, its section label
//! comes from its own name, and its term page is claimed by `<field>_page:`.

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

/// A fresh site in a temp folder, built once. Returns the guard and the
/// build's staging directory.
fn build_site(name: &str, files: &[(&str, &str)]) -> (Cleanup, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("moss_declared_{name}_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    for (rel, body) in files {
        write(&tmp, rel, body);
    }
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    (cleanup, tmp.join(".moss/build.nosync/staging"))
}

const PERFORMERS_CONFIG: &str = "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.performers]\nfields = [\"conductor\", \"soloist\"]\ntitle = \"Performers\"\n";

#[test]
fn declared_fields_build_one_term_page_with_a_section_per_field() {
    let (_guard, out) = build_site("sections", &[
        (".moss/config.toml", PERFORMERS_CONFIG),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("concerts/spring.md", "---\ntitle: Spring Concert\nconductor: Ada Example\nsoloist: Ben Sample\n---\n\nSpring.\n"),
        ("concerts/autumn.md", "---\ntitle: Autumn Concert\nsoloist: Ada Example\n---\n\nAutumn.\n"),
    ]);

    let page = fs::read_to_string(out.join("performers/ada-example/index.html"))
        .expect("the declared kind must give the person a term page");
    let conductor = page.find("Conductor").expect("a Conductor section");
    let soloist = page.find("Soloist").expect("a Soloist section");
    assert!(conductor < soloist, "Conductor comes before Soloist, the declared order:\n{page}");
    let spring = page.find("Spring Concert").expect("the conductor page is listed");
    let autumn = page.find("Autumn Concert").expect("the soloist page is listed");
    assert!(conductor < spring && spring < soloist && soloist < autumn, "each page sits under its own field:\n{page}");
}

#[test]
fn a_declared_field_is_not_refused_as_unrecognized() {
    let (_guard, out) = build_site("accepted", &[
        (".moss/config.toml", PERFORMERS_CONFIG),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("concerts/spring.md", "---\ntitle: Spring Concert\nconductor: Ada Example\n---\n\nSpring.\n"),
    ]);
    let page = fs::read_to_string(out.join("performers/ada-example/index.html")).unwrap();
    assert!(page.contains("Spring Concert"), "{page}");
}

#[test]
fn a_declared_field_claim_makes_the_page_the_persons_page() {
    let (_guard, out) = build_site("claim", &[
        (".moss/config.toml", PERFORMERS_CONFIG),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("concerts/spring.md", "---\ntitle: Spring Concert\nconductor: Ada Example\n---\n\nSpring.\n"),
        ("people/ada.md", "---\ntitle: Ada Example\nconductor_page: true\n---\n\nAbout Ada.\n"),
    ]);

    let stub = fs::read_to_string(out.join("performers/ada-example/index.html"))
        .expect("the generated address forwards to the claiming page");
    assert!(stub.contains("url=/people/ada/"), "the stub points at the claiming page:\n{stub}");
    assert!(
        !stub.contains("moss-term-role") && !stub.contains("Spring Concert"),
        "the generated address is a redirect, not a second listing:\n{stub}"
    );
    let claimed = fs::read_to_string(out.join("people/ada/index.html")).expect("the claiming page is built");
    assert!(claimed.contains("Spring Concert"), "the claiming page hosts the listing:\n{claimed}");
}

#[test]
fn an_undeclared_site_gets_no_performer_pages_and_keeps_author_pages() {
    let (_guard, out) = build_site("undeclared", &[
        (".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n"),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("concerts/spring.md", "---\ntitle: Spring Concert\nconductor: Ada Example\nauthor: Ben Sample\n---\n\nSpring.\n"),
    ]);
    assert!(!out.join("performers").exists(), "no declaration, no performers namespace");
    assert!(!out.join("conductors").exists());
    assert!(out.join("authors/ben-sample/index.html").exists(), "author: still feeds /authors/");
    let page = fs::read_to_string(out.join("concerts/spring/index.html")).unwrap();
    assert!(!page.contains("Conductor"), "an undeclared field gets no section:\n{page}");
}
