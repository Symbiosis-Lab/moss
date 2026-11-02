//! A declared field with `card = true` is drawn, as plain text, on every
//! listing card and row of a page that has a value for it.

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
    let tmp =
        std::env::temp_dir().join(format!("moss_card_fields_{name}_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let cleanup = Cleanup(tmp.clone());
    for (rel, body) in files {
        write(&tmp, rel, body);
    }
    let result = build_sync(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");
    (cleanup, tmp.join(".moss/build.nosync/staging"))
}

const CONFIG: &str = r#"schema_version = 6

[site]
lang = "en"

[fields.holder]
type = "text"
card = true

[fields.stage]
type = "choice"
card = true
of = { Wanted = "Wanted, in print", Held = "Held" }

[fields.note]
type = "text"
place = "facts"
"#;

fn site(config: &str) -> (Cleanup, PathBuf) {
    build_site("fields", &[
        (".moss/config.toml", config),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("described/index.md", "---\ntitle: Described\n---\n\nDescribed.\n"),
        ("described/a.md", "---\ntitle: Record A\ndate: 2026-10-01\ndescription: A rich card.\nholder: Example Archive\nstage: Wanted\nnote: Private\n---\n\nA.\n"),
        ("described/b.md", "---\ntitle: Record B\ndate: 2026-10-02\ndescription: Another.\nholder: \"R & <D>\"\n---\n\nB.\n"),
        ("described/c.md", "---\ntitle: Record C\ndate: 2026-10-03\ndescription: Third.\nnote: Private\n---\n\nC.\n"),
        ("dated/index.md", "---\ntitle: Dated\n---\n\nDated.\n"),
        ("dated/a.md", "---\ntitle: Record A\ndate: 2026-10-01\nholder: Example Archive\nstage: Wanted\nnote: Private\n---\n\nA.\n"),
        ("dated/b.md", "---\ntitle: Record B\ndate: 2026-10-02\nholder: \"R & <D>\"\n---\n\nB.\n"),
        ("dated/c.md", "---\ntitle: Record C\ndate: 2026-10-03\nnote: Private\n---\n\nC.\n"),
        ("bare/index.md", "---\ntitle: Bare\n---\n\nBare.\n"),
        ("bare/a.md", "---\ntitle: Record A\nholder: Example Archive\nstage: Wanted\nnote: Private\n---\n"),
        ("bare/b.md", "---\ntitle: Record B\nholder: \"R & <D>\"\n---\n"),
        ("bare/c.md", "---\ntitle: Record C\nnote: Private\n---\n"),
    ])
}

const HOLDER_A: &str = r#"<span class="moss-card-field" data-field="holder" data-value="Example Archive">Example Archive</span>"#;
const STAGE_A: &str = r#"<span class="moss-card-field" data-field="stage" data-value="wanted">Wanted, in print</span>"#;

#[test]
fn a_card_field_is_drawn_on_summary_and_grid_cards_in_declaration_order() {
    let (_guard, out) = site(CONFIG);
    for folder in ["described", "dated", "bare"] {
        let page = fs::read_to_string(out.join(folder).join("index.html")).unwrap();
        let holder = page
            .find(HOLDER_A)
            .unwrap_or_else(|| panic!("{folder} draws the holder:\n{page}"));
        let stage = page.find(STAGE_A).unwrap_or_else(|| {
            panic!("{folder} draws the choice by its config value and label:\n{page}")
        });
        assert!(
            holder < stage,
            "declaration order, holder before stage:\n{page}"
        );
    }
}

#[test]
fn a_card_field_value_is_escaped_and_a_field_without_card_is_absent() {
    let (_guard, out) = site(CONFIG);
    for folder in ["described", "dated", "bare"] {
        let page = fs::read_to_string(out.join(folder).join("index.html")).unwrap();
        assert!(
            page.contains(r#"data-value="R &amp; &lt;D&gt;">R &amp; &lt;D&gt;</span>"#),
            "{folder}:\n{page}"
        );
        assert!(
            !page.contains(r#"data-field="note""#),
            "{folder} draws no field without card:\n{page}"
        );
        assert!(
            !page.contains("Private"),
            "{folder} draws no field without card:\n{page}"
        );
    }
}

#[test]
fn a_page_without_a_value_draws_no_card_field_element() {
    let (_guard, out) = site(CONFIG);
    let page = fs::read_to_string(out.join("bare/index.html")).unwrap();
    // A, B carry holder; only A carries stage; C carries neither.
    assert_eq!(page.matches(r#"data-field="holder""#).count(), 2, "{page}");
    assert_eq!(page.matches(r#"data-field="stage""#).count(), 1, "{page}");
    assert_eq!(page.matches("moss-card-field").count(), 3, "{page}");
}

#[test]
fn card_fields_keep_the_field_place_on_the_page_itself() {
    let (_guard, out) =
        site(&CONFIG.replace("card = true\nof", "card = true\nplace = \"eyebrow\"\nof"));
    let page = fs::read_to_string(out.join("bare/a/index.html")).unwrap();
    assert!(
        page.contains(r#"moss-eyebrow"#) && page.contains(r#"data-field="stage""#),
        "{page}"
    );
    assert!(
        !page.contains("moss-card-field"),
        "the page itself carries no card field:\n{page}"
    );
}

#[test]
fn without_card_no_field_is_drawn_on_cards() {
    let (_guard, out) = site(&CONFIG.replace("card = true\n", ""));
    let page = fs::read_to_string(out.join("bare/index.html")).unwrap();
    assert!(!page.contains("moss-card-field"), "{page}");
}

#[test]
fn card_takes_a_boolean() {
    let (decls, problems) = moss_build::config::ConfigFile::parse(
        &CONFIG.replace("card = true\nof", "card = \"yes\"\nof"),
    )
    .unwrap()
    .field_decls();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("[fields.stage]") && p.contains("`card` must be true or false")),
        "{problems:?}"
    );
    assert!(!decls.iter().find(|d| d.name == "stage").unwrap().card);
}
