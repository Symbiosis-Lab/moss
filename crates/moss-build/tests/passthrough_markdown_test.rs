//! A pre-built app in a subfolder keeps its own `index.html`. Markdown beside
//! it (the app's README, licence, notes) must not claim the folder address, and
//! two sources that would write one address are reported.
//!
//! Its own test binary: `log::set_logger` is process-global and one-shot, and
//! the problem count is process-wide, so the tests take turns.

use moss_build::build::cli_output::take_cli_problems;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

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
static TURN: Mutex<()> = Mutex::new(());
static LOGGER: Once = Once::new();

const APP: &str = "<!doctype html><title>The App</title><p>app bytes</p>";

fn build_sync(folder: &Path) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(folder),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder.to_string_lossy()),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

struct Site {
    root: PathBuf,
    warnings: Vec<String>,
    problems: usize,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Site {
    fn out(&self, rel: &str) -> PathBuf {
        self.root.join(".moss/build.nosync/staging").join(rel)
    }
    fn warned(&self, parts: &[&str]) -> bool {
        self.warnings.iter().any(|w| parts.iter().all(|p| w.contains(p)))
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn build(config_extra: &str, files: &[(&str, &str)]) -> Site {
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    LOGGER.call_once(|| {
        log::set_logger(&CAPTURE).unwrap();
        log::set_max_level(log::LevelFilter::Warn);
    });
    let root = std::env::temp_dir().join(format!("moss_passthrough_md_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut all = vec![(".moss/config.toml", format!("schema_version = 6\n\n[site]\nlang = \"en\"\n{config_extra}"))];
    all.extend(files.iter().map(|(p, b)| (*p, b.to_string())));
    for (rel, body) in &all {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    CAPTURE.0.lock().unwrap().clear();
    let _ = take_cli_problems();
    let built = build_sync(&root);
    assert!(built.is_ok(), "build failed: {built:?}");
    Site { warnings: CAPTURE.0.lock().unwrap().clone(), problems: take_cli_problems(), root, _turn: turn }
}

const APP_FILES: [(&str, &str); 7] = [
    ("index.md", "---\ntitle: Home\n---\n\nHello.\n"),
    ("app/index.html", APP),
    ("app/app.js", "console.log(1)"),
    ("app/README.md", "# App readme\n\nReadme text.\n"),
    ("app/README.en.md", "# App readme in English\n\nEnglish text.\n"),
    ("app/notes.md", "# Notes\n\nNotes text. [English readme](README.en.md)\n"),
    ("app/vendor/kit/LICENSE.md", "# Licence\n\nLicence text.\n"),
];

#[test]
fn app_index_survives_its_markdown_and_the_readme_gets_its_own_address() {
    let site = build("", &APP_FILES);

    assert_eq!(fs::read_to_string(site.out("app/index.html")).unwrap(), APP);
    let readmes: String = ["app/readme/index.html", "app/readme-2/index.html"]
        .iter()
        .map(|p| fs::read_to_string(site.out(p)).unwrap())
        .collect();
    assert!(readmes.contains("Readme text.") && readmes.contains("English text."));
    let notes = fs::read_to_string(site.out("app/notes/index.html")).unwrap();
    assert!(notes.contains("Notes text."));
    assert!(notes.contains(r#"href="../readme/""#), "a link to a README must not point at the app: {notes}");
    assert!(fs::read_to_string(site.out("app/vendor/kit/license/index.html")).unwrap().contains("Licence text."));
    assert!(
        site.warned(&["app/README.md", "app/index.html"]),
        "expected a warning naming both files, got: {:?}",
        site.warnings
    );
    assert_eq!(site.problems, 0, "an app shipping a README must not fail --strict");
}

#[test]
fn declaring_the_app_a_passthrough_root_keeps_its_index_too() {
    let site = build("\n[build]\npassthrough = [\"app\"]\n", &APP_FILES);

    assert_eq!(fs::read_to_string(site.out("app/index.html")).unwrap(), APP);
    assert!(site.out("app/readme/index.html").exists() || site.out("app/readme-2/index.html").exists());
    assert!(site.warned(&["app/README.md", "app/index.html"]), "got: {:?}", site.warnings);
}

#[test]
fn a_marked_home_inside_the_app_does_not_take_the_folder_address_either() {
    let site = build(
        "",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nHello.\n"),
            ("app/index.html", APP),
            ("app/about.md", "---\nhome: true\n---\n\nAbout text.\n"),
        ],
    );

    assert_eq!(fs::read_to_string(site.out("app/index.html")).unwrap(), APP);
    assert!(fs::read_to_string(site.out("app/about/index.html")).unwrap().contains("About text."));
}

#[test]
fn a_page_keeps_its_address_over_a_nested_app_page_and_the_clash_is_a_problem() {
    let site = build(
        "",
        &[
            ("index.md", "---\ntitle: Home\n---\n\nHello.\n"),
            ("app/index.html", APP),
            ("app/docs/index.html", "<!doctype html><p>nested app</p>"),
            ("app/docs.md", "# Docs\n\nPage text.\n"),
        ],
    );

    assert!(fs::read_to_string(site.out("app/docs/index.html")).unwrap().contains("Page text."));
    assert!(
        site.warned(&["app/docs.md", "app/docs/index.html", "/app/docs/", "the page is served"]),
        "got: {:?}",
        site.warnings
    );
    assert!(site.problems >= 1, "silently dropping a file is a problem");
}

fn sibling_page_keeps_the_address(config_extra: &str) {
    let site = build(
        config_extra,
        &[
            ("index.md", "---\ntitle: Home\n---\n\nHello.\n"),
            ("app/index.html", APP),
            ("app.md", "# App page\n\nSibling text.\n"),
        ],
    );

    assert!(fs::read_to_string(site.out("app/index.html")).unwrap().contains("Sibling text."));
    assert!(site.warned(&["app.md", "app/index.html", "/app/", "the page is served"]), "got: {:?}", site.warnings);
    assert!(site.problems >= 1);
}

#[test]
fn a_sibling_page_keeps_the_app_address_it_always_had() {
    sibling_page_keeps_the_address("");
}

#[test]
fn a_sibling_page_keeps_the_app_address_with_the_app_declared_passthrough() {
    sibling_page_keeps_the_address("\n[build]\npassthrough = [\"app\"]\n");
}

#[test]
fn a_page_and_a_loose_html_file_at_one_address_are_reported_by_name() {
    let site = build(
        "",
        &[("index.md", "---\ntitle: Home\n---\n\nHello.\n"), ("index.html", "<!doctype html><p>loose</p>")],
    );

    assert!(fs::read_to_string(site.out("index.html")).unwrap().contains("Hello."));
    assert!(site.warned(&["index.md", "`index.html`", "/"]), "got: {:?}", site.warnings);
    assert!(site.problems >= 1);
}

#[test]
fn a_page_embeds_an_image_from_a_passthrough_folder_at_its_copied_path() {
    let site = build(
        "",
        &[
            ("index.md", "---\ntitle: Home\n---\n\n![A photo](app/photo.jpg)\n"),
            ("app/index.html", APP),
            ("app/photo.jpg", "fake-jpg"),
        ],
    );

    assert_eq!(fs::read(site.out("app/photo.jpg")).unwrap(), b"fake-jpg");
    let home = fs::read_to_string(site.out("index.html")).unwrap();
    assert!(home.contains(r#"src="/app/photo.jpg""#), "the embed must point at the copied file: {home}");
}
