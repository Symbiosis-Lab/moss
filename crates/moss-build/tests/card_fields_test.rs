//! A declared field with `card = true` is drawn on every listing card and
//! row of a page that has a value for it: never a link, and a text value with
//! only inline emphasis.

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

fn span(field: &str, value: &str) -> String {
    format!(
        r#"<span class="moss-card-field" data-field="{field}" data-value="{value}">{value}</span>"#
    )
}

#[test]
fn list_rows_draw_card_fields_for_dated_dateless_and_event_pages() {
    let (_guard, out) = build_site(
        "rows",
        &[
            (".moss/config.toml", CONFIG),
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            (
                "rows/index.md",
                "---\ntitle: Rows\nchildren_style: list\nchildren_group: none\n---\n\nRows.\n",
            ),
            (
                "rows/dated.md",
                "---\ntitle: Dated Row\ndate: 2026-10-01\nholder: HDated\n---\n\nA.\n",
            ),
            (
                "rows/dateless.md",
                "---\ntitle: Dateless Row\nholder: HDateless\n---\n\nB.\n",
            ),
            (
                "rows/event.md",
                "---\ntitle: Event Row\nstart: 2099-03-01 19:30\nholder: HEvent\n---\n\nC.\n",
            ),
            ("rows/empty.md", "---\ntitle: Empty Row\n---\n\nD.\n"),
        ],
    );
    let page = fs::read_to_string(out.join("rows/index.html")).unwrap();
    for holder in ["HDated", "HDateless", "HEvent"] {
        assert!(
            page.contains(&span("holder", holder)),
            "{holder} row draws its field:\n{page}"
        );
    }
    assert_eq!(
        page.matches("moss-card-field").count(),
        3,
        "a row without a value draws none:\n{page}"
    );
}

#[test]
fn a_card_field_follows_the_term_credits_with_one_separator() {
    let config = format!("{CONFIG}\n[terms.performers]\nfields = [\"conductor\"]\ntitle = \"Performers\"\ncards = true\n");
    let (_guard, out) = build_site(
        "joined",
        &[
            (".moss/config.toml", &config),
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("bare/index.md", "---\ntitle: Bare\n---\n\nBare.\n"),
            (
                "bare/gala.md",
                "---\ntitle: Gala\nconductor: Ada Example\nholder: HGala\n---\n",
            ),
        ],
    );
    let page = fs::read_to_string(out.join("bare/index.html")).unwrap();
    let joined = format!(
        r#"</span></span><span class="moss-event-sep"> · </span>{}"#,
        span("holder", "HGala")
    );
    assert!(
        page.contains(&joined),
        "credits, one separator, then the field:\n{page}"
    );
    assert_eq!(page.matches("moss-event-sep").count(), 1, "{page}");
}

#[test]
fn card_on_a_group_warns_and_draws_nothing() {
    let config = format!("{CONFIG}\n[fields.line]\ntype = \"group\"\nof = [\"holder\"]\nplace = \"eyebrow\"\ncard = true\n");
    let (decls, problems) = moss_build::config::ConfigFile::parse(&config)
        .unwrap()
        .field_decls();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("[fields.line]") && p.contains("group")),
        "{problems:?}"
    );
    assert!(!decls.iter().find(|d| d.name == "line").unwrap().card);
    let (_guard, out) = build_site(
        "group",
        &[
            (
                ".moss/config.toml",
                &config.replace(
                    "[fields.holder]\ntype = \"text\"\ncard = true",
                    "[fields.holder]\ntype = \"text\"",
                ),
            ),
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("bare/index.md", "---\ntitle: Bare\n---\n\nBare.\n"),
            ("bare/a.md", "---\ntitle: A\nholder: HA\n---\n"),
        ],
    );
    let page = fs::read_to_string(out.join("bare/index.html")).unwrap();
    assert!(!page.contains("moss-card-field"), "{page}");
}

const JOURNAL_CONFIG: &str = r#"schema_version = 6

[site]
lang = "en"

[fields.journal]
type = "text"
card = true
place = "facts"
"#;

fn journal_site() -> (Cleanup, PathBuf) {
    build_site(
        "journal",
        &[
            (".moss/config.toml", JOURNAL_CONFIG),
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("papers/index.md", "---\ntitle: Papers\nchildren_style: list\nchildren_group: none\n---\n\nPapers.\n"),
            ("papers/a.md", "---\ntitle: Paper A\njournal: \"*Example Journal of Gardens,* 4(35): 1-7\"\n---\n\nA.\n"),
            ("papers/b.md", "---\ntitle: Paper B\njournal: \"**Bold** and `code` in [Example Press](https://example.test/press)\"\n---\n\nB.\n"),
            ("papers/d.md", "---\ntitle: Paper D\njournal: a_b_c\n---\n\nD.\n"),
            ("papers/e.md", "---\ntitle: Paper E\njournal: \"2* and 3*\"\n---\n\nE.\n"),
            ("papers/c.md", "---\ntitle: Paper C\njournal: \"<script>alert(1)</script> _Quiet_\"\n---\n\nC.\n"),
        ],
    )
}

fn card_field(page: &str, source: &str) -> String {
    let start = page.find(&format!(r#"data-source-file="{source}""#)).unwrap_or_else(|| panic!("{source}:\n{page}"));
    let rest = &page[start..];
    let open = rest.find(r#"<span class="moss-card-field""#).unwrap_or_else(|| panic!("no card field after {source}:\n{page}"));
    let rest = &rest[open..];
    rest[..rest.find("</span>").unwrap() + 7].to_string()
}

#[test]
fn a_text_card_field_keeps_inline_emphasis_and_a_plain_data_value() {
    let (_guard, out) = journal_site();
    let page = fs::read_to_string(out.join("papers/index.html")).unwrap();
    assert_eq!(
        card_field(&page, "papers/a.md"),
        r#"<span class="moss-card-field" data-field="journal" data-value="Example Journal of Gardens, 4(35): 1-7"><em>Example Journal of Gardens,</em> 4(35): 1-7</span>"#,
        "{page}"
    );
}

#[test]
fn a_card_field_link_keeps_its_words_and_drops_its_target() {
    let (_guard, out) = journal_site();
    let page = fs::read_to_string(out.join("papers/index.html")).unwrap();
    assert_eq!(
        card_field(&page, "papers/b.md"),
        r#"<span class="moss-card-field" data-field="journal" data-value="Bold and code in Example Press"><strong>Bold</strong> and <code>code</code> in Example Press</span>"#,
        "{page}"
    );
    assert!(!page.contains("example.test/press"), "{page}");
}

#[test]
fn a_card_field_script_stays_escaped_text() {
    let (_guard, out) = journal_site();
    let page = fs::read_to_string(out.join("papers/index.html")).unwrap();
    assert_eq!(
        card_field(&page, "papers/c.md"),
        r#"<span class="moss-card-field" data-field="journal" data-value="&lt;script&gt;alert(1)&lt;/script&gt; Quiet">&lt;script&gt;alert(1)&lt;/script&gt; <em>Quiet</em></span>"#,
        "{page}"
    );
    assert!(!page.contains("<script>alert"), "{page}");
}

#[test]
fn a_placed_text_field_reads_the_same_emphasis_on_its_page() {
    let (_guard, out) = journal_site();
    let page = fs::read_to_string(out.join("papers/a/index.html")).unwrap();
    assert!(
        page.contains("<dd><em>Example Journal of Gardens,</em> 4(35): 1-7</dd>"),
        "{page}"
    );
}

#[test]
fn text_that_is_not_emphasis_stays_as_written() {
    let (_guard, out) = journal_site();
    let page = fs::read_to_string(out.join("papers/index.html")).unwrap();
    assert_eq!(
        card_field(&page, "papers/d.md"),
        r#"<span class="moss-card-field" data-field="journal" data-value="a_b_c">a_b_c</span>"#,
        "{page}"
    );
    assert_eq!(
        card_field(&page, "papers/e.md"),
        r#"<span class="moss-card-field" data-field="journal" data-value="2* and 3*">2* and 3*</span>"#,
        "{page}"
    );
}
