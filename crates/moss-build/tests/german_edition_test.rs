//! German is a site edition: a page under a `de/` tree is dressed in German
//! interface strings, German dates and `<html lang="de">`, and the switcher on
//! the other editions reads "DE", as the English one reads "EN".

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn build_sync(folder_path: &str, trigger: moss_build::build::BuildTrigger) -> Result<String, String> {
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);
    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

struct Site(PathBuf);

impl Site {
    fn new(files: &[(&str, &str)]) -> Site {
        let root = std::env::temp_dir().join(format!("moss_german_edition_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let site = Site(root);
        site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
        for (rel, body) in files {
            site.write(rel, body);
        }
        let result = build_sync(&site.0.to_string_lossy(), moss_build::build::BuildTrigger::Full);
        assert!(result.is_ok(), "build failed: {result:?}");
        site
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn html(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(".moss/build.nosync/staging").join(rel)).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_de_tree_is_dressed_in_german_chrome_and_dates() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ("de/blog/index.md", "---\ntitle: Blog\n---\n\nText.\n"),
        ("de/blog/erster.md", "---\ntitle: Erster\ndate: 2026-10-15\n---\n\n## Anfang\n\nText.\n"),
        ("de/blog/zweiter.md", "---\ntitle: Zweiter\ndate: 2026-10-16\n---\n\nText.\n"),
    ]);

    let post = site.html("de/blog/erster/index.html");
    assert!(post.contains(r#"<html data-moss-html-version="1" lang="de">"#), "{post}");
    assert!(post.contains("Zum Inhalt springen"), "skip link");
    assert!(post.contains(r#"aria-label="Sprache""#), "language switcher label");
    assert!(post.contains(r#"aria-label="Leseeinstellungen""#), "reading preferences");
    assert!(post.contains(">15. Oktober 2026<"), "formatted date");
    assert!(post.contains(r#"aria-label="Dauerhafter Link zu diesem Abschnitt""#), "heading anchor");

    let home = site.html("de/index.html");
    assert!(home.contains("2 Artikel"), "folder card count: {home}");
    assert!(home.contains("Veröffentlicht mit moss"), "colophon");
    assert!(!home.contains("Skip to content"), "no English chrome on a German page");

    // The English edition keeps its own chrome and points at the German one.
    let root = site.html("index.html");
    assert!(root.contains("Skip to content") && root.contains(r#"hreflang="de""#), "{root}");
    assert!(root.contains(r#"hreflang="de">DE</a>"#), "the switcher labels the German edition DE, as it labels English EN: {root}");
}

#[test]
fn a_regional_german_tree_keeps_its_region_beside_the_plain_german_one() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("about.md", "---\ntitle: About\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ("de/about.md", "---\ntitle: Über\n---\n\nText.\n"),
        ("de-at/index.md", "---\ntitle: Servus\n---\n\nWillkommen.\n"),
        ("de-at/about.md", "---\ntitle: Über uns\n---\n\nText.\n"),
    ]);

    let de = site.html("de/about/index.html");
    assert!(de.contains(r#"<html data-moss-html-version="1" lang="de">"#), "{de}");
    assert!(de.contains("Zum Inhalt springen"), "German chrome in de/");

    let at = site.html("de-at/about/index.html");
    assert!(at.contains(r#"<html data-moss-html-version="1" lang="de-AT">"#), "{at}");
    assert!(at.contains("Zum Inhalt springen"), "German chrome in de-at/");

    let root = site.html("about/index.html");
    assert!(root.contains(r#"hreflang="de""#), "plain German alternate: {root}");
    assert!(root.contains(r#"hreflang="de-AT""#), "regional German alternate: {root}");
}
